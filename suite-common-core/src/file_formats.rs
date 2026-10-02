// file_formats.rs — the file formats an app opens and saves, declared once.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Each app's readers and writers dispatch on the extension, its Open dialog
// lists patterns, its desktop entry tells the file manager which MIME types
// to offer it for, and docs/FORMATS.md tells people. Those four used to be
// written separately and disagreed: Tables' loader claimed `.xlsb`, which it
// cannot parse, its Open dialog omitted `.xlsm` and `.tsv`, which it can, and
// Decks wrote PowerPoint bytes under any name that wasn't `.odp` (#1206).
//
// Each core crate now declares a `FORMATS` table of these, and its tests hold
// the desktop entry and docs/FORMATS.md to it; the GUI builds its Open filter
// from it.

/// One file format: what it is called, the extensions that select it, the
/// MIME type a desktop entry advertises for it, and whether the app writes it
/// as well as reading it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileFormat {
    pub label: &'static str,
    /// The first is the one a save writes; the rest are accepted spellings.
    pub extensions: &'static [&'static str],
    pub mime: &'static str,
    pub writable: bool,
}

impl FileFormat {
    /// The extension a save dialog pre-fills.
    pub fn extension(&self) -> &'static str {
        self.extensions[0]
    }

    pub fn matches_extension(&self, extension: &str) -> bool {
        self.extensions.iter().any(|known| known.eq_ignore_ascii_case(extension))
    }
}

/// The format `path`'s extension selects, if any.
pub fn for_path<'a>(formats: &'a [FileFormat], path: &str) -> Option<&'a FileFormat> {
    let extension = std::path::Path::new(path).extension()?.to_str()?;
    formats.iter().find(|format| format.matches_extension(extension))
}

/// `*.ext` for every extension of every format, for an Open dialog's filter.
pub fn open_patterns(formats: &[FileFormat]) -> Vec<String> {
    formats
        .iter()
        .flat_map(|format| format.extensions.iter().map(|extension| format!("*.{extension}")))
        .collect()
}

/// The MIME types a desktop entry's `MimeType=` key lists.
pub fn desktop_mime_types(desktop_entry: &str) -> Vec<&str> {
    desktop_entry
        .lines()
        .find_map(|line| line.strip_prefix("MimeType="))
        .map(|list| list.split(';').map(str::trim).filter(|mime| !mime.is_empty()).collect())
        .unwrap_or_default()
}

/// The rows docs/FORMATS.md states under `## <app>`, as
/// (label, extensions, MIME type, opens, saves). The table's columns are
/// `| Format | Extensions | MIME type | Opens | Saves |`, with extensions as
/// `` `.odt` `` code spans and "yes"/"no" in the last two.
pub fn documented_formats(markdown: &str, app: &str) -> Vec<(String, Vec<String>, String, bool, bool)> {
    let heading = format!("## {app}");
    let Some(start) = markdown.lines().position(|line| line.trim() == heading) else {
        return Vec::new();
    };
    markdown
        .lines()
        .skip(start + 1)
        .take_while(|line| !line.starts_with("## "))
        .filter(|line| line.starts_with('|') && !line.starts_with("|---") && !line.starts_with("| Format"))
        .filter_map(|line| {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
            let [label, extensions, mime, opens, saves] = cells.as_slice() else {
                return None;
            };
            let extensions = extensions
                .split(',')
                .map(|extension| extension.trim().trim_matches('`').trim_start_matches('.').to_string())
                .collect();
            Some((
                label.to_string(),
                extensions,
                mime.trim_matches('`').to_string(),
                *opens == "yes",
                *saves == "yes",
            ))
        })
        .collect()
}

/// Every way `formats` disagrees with the app's desktop entry and with its
/// section of docs/FORMATS.md; empty when all three say the same thing.
pub fn disagreements(formats: &[FileFormat], app: &str, desktop_entry: &str, docs: &str) -> Vec<String> {
    let mut problems = Vec::new();

    let mut declared: Vec<&str> = formats.iter().map(|format| format.mime).collect();
    let mut advertised = desktop_mime_types(desktop_entry);
    declared.sort_unstable();
    advertised.sort_unstable();
    if declared != advertised {
        problems.push(format!(
            "the desktop entry's MimeType lists {advertised:?}, but the formats the app opens are {declared:?}"
        ));
    }

    let documented = documented_formats(docs, app);
    if documented.is_empty() {
        problems.push(format!("docs/FORMATS.md has no `## {app}` table"));
    }
    for format in formats {
        match documented.iter().find(|row| row.0 == format.label) {
            None => problems.push(format!("docs/FORMATS.md does not list {} under {app}", format.label)),
            Some((_, extensions, mime, opens, saves)) => {
                let expected: Vec<String> = format.extensions.iter().map(|e| e.to_string()).collect();
                if *extensions != expected || mime != format.mime || !opens || *saves != format.writable {
                    problems.push(format!(
                        "docs/FORMATS.md says {} is {extensions:?} {mime} opens={opens} saves={saves}; \
                         the app declares {expected:?} {} opens=true saves={}",
                        format.label, format.mime, format.writable
                    ));
                }
            }
        }
    }
    for row in &documented {
        if !formats.iter().any(|format| format.label == row.0) {
            problems.push(format!("docs/FORMATS.md lists {} under {app}, which the app does not declare", row.0));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMATS: &[FileFormat] = &[
        FileFormat { label: "Thing", extensions: &["thg", "thing"], mime: "application/x-thing", writable: true },
        FileFormat { label: "Old thing", extensions: &["oth"], mime: "application/x-old-thing", writable: false },
    ];
    const DESKTOP: &str = "[Desktop Entry]\nName=Thing\nMimeType=application/x-thing;application/x-old-thing;\n";
    const DOCS: &str = "# Formats\n\n## Thing\n\n| Format | Extensions | MIME type | Opens | Saves |\n|---|---|---|---|---|\n\
        | Thing | `.thg`, `.thing` | `application/x-thing` | yes | yes |\n\
        | Old thing | `.oth` | `application/x-old-thing` | yes | no |\n\n## Other\n\n| Format | Extensions | MIME type | Opens | Saves |\n";

    #[test]
    fn a_path_finds_its_format_by_any_spelling_in_any_case() {
        assert_eq!(for_path(FORMATS, "/a/b.THING").map(|f| f.label), Some("Thing"));
        assert_eq!(for_path(FORMATS, "b.oth").map(|f| f.label), Some("Old thing"));
        assert_eq!(for_path(FORMATS, "b.txt"), None);
        assert_eq!(for_path(FORMATS, "no-extension"), None);
        assert_eq!(open_patterns(FORMATS), ["*.thg", "*.thing", "*.oth"]);
    }

    #[test]
    fn agreeing_sources_have_no_disagreements() {
        assert_eq!(disagreements(FORMATS, "Thing", DESKTOP, DOCS), Vec::<String>::new());
    }

    #[test]
    fn each_kind_of_drift_is_reported() {
        let missing_mime = DESKTOP.replace("application/x-old-thing;", "");
        assert_eq!(disagreements(FORMATS, "Thing", &missing_mime, DOCS).len(), 1);
        let says_it_saves = DOCS.replace("| yes | no |", "| yes | yes |");
        assert_eq!(disagreements(FORMATS, "Thing", DESKTOP, &says_it_saves).len(), 1);
        let undeclared = DOCS.replace("\n\n## Other", "\n| Extra | `.x` | `a/x` | yes | no |\n\n## Other");
        assert_eq!(disagreements(FORMATS, "Thing", DESKTOP, &undeclared).len(), 1);
        assert!(!disagreements(FORMATS, "Missing", DESKTOP, DOCS).is_empty());
    }
}
