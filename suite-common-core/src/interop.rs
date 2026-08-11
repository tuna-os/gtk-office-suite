//! Structured compatibility reports and safe opaque-package preservation.
//!
//! Format readers use this module to report what they recognised and what
//! they cannot interpret.  The report is deliberately data, not dialog copy:
//! GTK callers can render it, tests can assert it, and headless imports can
//! still make the same save decision.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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
        let mut parts = BTreeMap::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|e| format!("read package entry: {e}"))?;
            let name = entry.name().to_string();
            if entry.is_dir() || recognized.iter().any(|known| *known == name) {
                continue;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| format!("read {name}: {e}"))?;
            parts.insert(name, bytes);
        }
        Ok(Self { parts })
    }

    pub fn is_empty(&self) -> bool { self.parts.is_empty() }
    pub fn part_names(&self) -> impl Iterator<Item = &str> { self.parts.keys().map(String::as_str) }
    pub fn len(&self) -> usize { self.parts.len() }

    /// Add captured members to a newly generated package. Existing generated
    /// members always win; this prevents stale opaque data from replacing an
    /// intentional user edit. A temporary archive avoids partial saves.
    pub fn append_to(&self, path: impl AsRef<Path>) -> Result<(), String> {
        if self.parts.is_empty() { return Ok(()); }
        let path = path.as_ref();
        let input = File::open(path).map_err(|e| format!("open generated package: {e}"))?;
        let mut source = ZipArchive::new(input).map_err(|e| format!("read generated package: {e}"))?;
        let existing: BTreeSet<String> = (0..source.len())
            .filter_map(|i| source.by_index(i).ok().map(|entry| entry.name().to_string()))
            .collect();
        let temporary = temporary_path(path);
        let output = File::create(&temporary).map_err(|e| format!("create temporary package: {e}"))?;
        let mut writer = ZipWriter::new(output);
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
        std::fs::rename(&temporary, path).map_err(|e| format!("replace package: {e}"))
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut temporary = path.to_path_buf();
    temporary.set_extension(format!("{}-opaque-tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("package")));
    temporary
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

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
}
