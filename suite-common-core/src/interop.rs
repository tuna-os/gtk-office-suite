//! Structured compatibility reports and safe opaque-package preservation.
//!
//! Format readers use this module to report what they recognised and what
//! they cannot interpret.  The report is deliberately data, not dialog copy:
//! GTK callers can render it, tests can assert it, and headless imports can
//! still make the same save decision.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use crate::atomic_save::atomic_write_bytes;
use crate::zip_guard::ZipBudget;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureDisposition {
    MustPreserve,
    OpaquePassThrough,
    WarnOnLoss,
    HardError,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UnsupportedFeature {
    pub id: String,
    pub label: String,
    pub location: String,
    pub disposition: FeatureDisposition,
    pub detail: String,
}

impl UnsupportedFeature {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        location: impl Into<String>,
        disposition: FeatureDisposition,
        detail: impl Into<String>,
    ) -> Self {
        Self { id: id.into(), label: label.into(), location: location.into(), disposition, detail: detail.into() }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompatibilityReport {
    pub format: String,
    pub features: Vec<UnsupportedFeature>,
    /// UI-safe alternatives shown before a destructive save. These are
    /// actions, not preformatted dialog text, so callers can localize them.
    pub safer_options: Vec<SaveOption>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveOption {
    KeepOriginal,
    SaveCopy,
    ExportAs { format: String },
}

impl CompatibilityReport {
    pub fn new(format: impl Into<String>) -> Self {
        Self { format: format.into(), features: Vec::new(), safer_options: vec![SaveOption::KeepOriginal, SaveOption::SaveCopy] }
    }

    pub fn record(&mut self, feature: UnsupportedFeature) {
        if !self.features.iter().any(|existing| existing.id == feature.id && existing.location == feature.location) {
            self.features.push(feature);
        }
    }

    pub fn has_hard_errors(&self) -> bool {
        self.features.iter().any(|f| f.disposition == FeatureDisposition::HardError)
    }

    pub fn needs_save_warning(&self) -> bool {
        self.features.iter().any(|f| matches!(f.disposition, FeatureDisposition::WarnOnLoss | FeatureDisposition::OpaquePassThrough | FeatureDisposition::HardError))
    }

    pub fn destructive_features(&self) -> Vec<&UnsupportedFeature> {
        self.features.iter().filter(|f| matches!(f.disposition, FeatureDisposition::WarnOnLoss | FeatureDisposition::HardError)).collect()
    }

    pub fn requires_confirmation(&self) -> bool {
        !self.destructive_features().is_empty()
    }

    /// Enforce the loss budget at the save boundary. A UI can show the
    /// report first and retry with `confirm_destructive = true` after the
    /// user explicitly chooses the lossy option.
    pub fn validate_save(&self, confirm_destructive: bool) -> Result<(), String> {
        if self.has_hard_errors() {
            return Err("save blocked: a supported or promised-pass-through feature cannot be preserved".into());
        }
        if self.requires_confirmation() && !confirm_destructive {
            return Err("save requires confirmation because unsupported content may be lost".into());
        }
        Ok(())
    }

    /// Stable, concise text for a toast/dialog; callers should use the
    /// structured fields above for decisions and tests.
    pub fn summary(&self) -> String {
        if self.features.is_empty() {
            return format!("No unsupported features detected in {}", self.format);
        }
        format!("{} unsupported feature(s) detected in {}", self.features.len(), self.format)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OpaquePackage {
    parts: BTreeMap<String, Vec<u8>>,
}

impl OpaquePackage {
    /// Capture package members not claimed by the format reader. The source
    /// bytes stay untouched so an unrelated edit can be saved without
    /// silently deleting an extension, custom XML part, or embedded object.
    pub fn capture(path: impl AsRef<Path>, recognized: &[&str]) -> Result<Self, String> {
        let file = File::open(path.as_ref()).map_err(|e| format!("open package: {e}"))?;
        let mut archive = ZipArchive::new(file).map_err(|e| format!("read package: {e}"))?;
        // Opaque capture reads every member this suite does not recognise,
        // which makes it the widest decompression surface in the codebase:
        // it is reached by all six `*_with_report` readers and it keeps what
        // it reads. It is bounded for the same reason (#442).
        let mut budget = ZipBudget::default();
        budget.check_entry_count(archive.len())?;
        let mut parts = BTreeMap::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|e| format!("read package entry: {e}"))?;
            let name = entry.name().to_string();
            if entry.is_dir() || recognized.iter().any(|known| *known == name) {
                continue;
            }
            let bytes = budget
                .read_entry(&mut entry, &name)
                .map_err(|e| format!("read {name}: {e}"))?;
            parts.insert(name, bytes);
        }
        Ok(Self { parts })
    }

    pub fn is_empty(&self) -> bool { self.parts.is_empty() }
    pub fn part_names(&self) -> impl Iterator<Item = &str> { self.parts.keys().map(String::as_str) }
    pub fn len(&self) -> usize { self.parts.len() }

    /// Add captured members to a newly generated package. Existing generated
    /// members always win; this prevents stale opaque data from replacing an
    /// intentional user edit.
    ///
    /// The rebuilt package is assembled in memory and handed to
    /// `atomic_write_bytes`, the same contract every format writer follows:
    /// this used to be the one save path that wrote its own temporary, and
    /// it did so at a fully predictable name with `File::create`, which
    /// follows symlinks. Going through `atomic_save` buys exclusive
    /// temporary creation, permission preservation, data and directory
    /// sync, and cleanup on failure — on a path that ends every
    /// opaque-preserving save.
    pub fn append_to(&self, path: impl AsRef<Path>) -> Result<(), String> {
        if self.parts.is_empty() { return Ok(()); }
        let path = path.as_ref();
        let input = File::open(path).map_err(|e| format!("open generated package: {e}"))?;
        let mut source = ZipArchive::new(input).map_err(|e| format!("read generated package: {e}"))?;
        let existing: BTreeSet<String> = (0..source.len())
            .filter_map(|i| source.by_index(i).ok().map(|entry| entry.name().to_string()))
            .collect();
        let mut buffer = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut buffer));
            let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for index in 0..source.len() {
                let mut entry = source.by_index(index).map_err(|e| format!("read generated entry: {e}"))?;
                if entry.is_dir() { continue; }
                let name = entry.name().to_string();
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).map_err(|e| format!("read generated {name}: {e}"))?;
                writer.start_file(&name, options).map_err(|e| format!("write generated {name}: {e}"))?;
                writer.write_all(&bytes).map_err(|e| format!("write generated {name}: {e}"))?;
            }
            for (name, bytes) in &self.parts {
                if existing.contains(name) { continue; }
                writer.start_file(name, options).map_err(|e| format!("write opaque {name}: {e}"))?;
                writer.write_all(bytes).map_err(|e| format!("write opaque {name}: {e}"))?;
            }
            writer.finish().map_err(|e| format!("finish package: {e}"))?;
        }
        atomic_write_bytes(path, &buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_structured_and_classifies_destructive_features() {
        let mut report = CompatibilityReport::new("docx");
        report.record(UnsupportedFeature::new("custom-xml", "Custom XML", "customXml/item1.xml", FeatureDisposition::OpaquePassThrough, "not interpreted"));
        report.record(UnsupportedFeature::new("macro", "VBA macro", "vbaProject.bin", FeatureDisposition::HardError, "cannot safely rewrite"));
        assert!(report.needs_save_warning());
        assert!(report.has_hard_errors());
        assert_eq!(report.destructive_features().len(), 1);
        assert_eq!(serde_json::to_value(&report.features[0]).unwrap()["disposition"], "opaque_pass_through");
    }

    #[test]
    fn save_policy_requires_confirmation_or_blocks_hard_error() {
        let mut warning = CompatibilityReport::new("xlsx");
        warning.record(UnsupportedFeature::new("pivot", "Pivot table", "xl/pivotTables/pivot1.xml", FeatureDisposition::WarnOnLoss, "not editable"));
        assert!(warning.requires_confirmation());
        assert!(warning.validate_save(false).is_err());
        assert!(warning.validate_save(true).is_ok());

        let mut blocked = CompatibilityReport::new("docx");
        blocked.record(UnsupportedFeature::new("macro", "VBA macro", "word/vbaProject.bin", FeatureDisposition::HardError, "promised pass-through cannot be guaranteed"));
        assert!(blocked.validate_save(true).is_err());
    }

    #[test]
    fn opaque_parts_survive_generated_package_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.docx");
        let output = dir.path().join("output.docx");
        for path in [&source, &output] {
            let file = File::create(path).unwrap();
            let mut zip = ZipWriter::new(file);
            zip.start_file("word/document.xml", SimpleFileOptions::default()).unwrap();
            zip.write_all(b"generated").unwrap();
            if path == &source {
                zip.start_file("customXml/item1.xml", SimpleFileOptions::default()).unwrap();
                zip.write_all(b"opaque extension").unwrap();
            }
            zip.finish().unwrap();
        }
        let opaque = OpaquePackage::capture(&source, &["word/document.xml"]).unwrap();
        opaque.append_to(&output).unwrap();
        let mut archive = ZipArchive::new(File::open(output).unwrap()).unwrap();
        let mut item = archive.by_name("customXml/item1.xml").unwrap();
        let mut bytes = Vec::new();
        item.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"opaque extension");
    }

    /// `append_to` used to create its temporary with `File::create` at a
    /// fully predictable name (`report.odt` → `report.odt-opaque-tmp`).
    /// `File::create` follows symlinks, so anything pre-created at that
    /// name was opened and truncated — the exact hazard `atomic_save` has
    /// guarded since #437, on the path every opaque-preserving save ends
    /// with.
    #[cfg(unix)]
    #[test]
    fn appending_opaque_parts_does_not_follow_a_stale_temporary_symlink() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("report.odt");
        let unrelated = dir.path().join("unrelated.txt");
        std::fs::write(&unrelated, b"must survive").unwrap();

        // A package with one recognised part, so capture has something to
        // keep and append_to has something to add.
        {
            let file = File::create(&package).unwrap();
            let mut writer = ZipWriter::new(file);
            writer.start_file("content.xml", SimpleFileOptions::default()).unwrap();
            writer.write_all(b"<x/>").unwrap();
            writer.start_file("extras/keep.bin", SimpleFileOptions::default()).unwrap();
            writer.write_all(b"opaque").unwrap();
            writer.finish().unwrap();
        }
        let opaque = OpaquePackage::capture(&package, &["content.xml"]).unwrap();
        assert_eq!(opaque.len(), 1);

        // Regenerate the package, then plant the symlink the old temporary
        // name would have opened.
        {
            let file = File::create(&package).unwrap();
            let mut writer = ZipWriter::new(file);
            writer.start_file("content.xml", SimpleFileOptions::default()).unwrap();
            writer.write_all(b"<x/>").unwrap();
            writer.finish().unwrap();
        }
        let stale = dir.path().join("report.odt-opaque-tmp");
        std::os::unix::fs::symlink(&unrelated, &stale).unwrap();

        opaque.append_to(&package).unwrap();

        assert_eq!(
            std::fs::read(&unrelated).unwrap(),
            b"must survive",
            "a save followed a symlink and truncated an unrelated file"
        );
        assert!(stale.is_symlink(), "the save removed a file it did not create");
        // And the opaque part still made it into the package.
        let restored = OpaquePackage::capture(&package, &["content.xml"]).unwrap();
        assert_eq!(restored.len(), 1);
    }
}
