// format.rs — which spreadsheet formats Tables can write, as opposed to read.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Tables imports xlsx/xlsm/xls/ods/csv/tsv but has exactly one writer:
// `save_sheets_to_xlsx`. Import support does not imply export support, and
// conflating the two destroys files — saving over an imported `.csv` writes
// an xlsx package under a `.csv` name, which nothing can reopen, least of all
// Tables' own csv reader (#439).
//
// Capability is therefore explicit and lives here, GTK-free, so every save
// path can consult the same answer.

/// Extensions `save_sheets_to_xlsx` produces a valid file for.
///
/// Deliberately just `xlsx`. `xlsm` and `xlsb` are excluded even though the
/// loader reads them: the writer emits a plain xlsx package, so writing it
/// under `.xlsm` would silently drop the macros that are the only reason the
/// file was `.xlsm`, and `.xlsb` is a different container entirely.
const WRITABLE_EXTENSIONS: [&str; 1] = ["xlsx"];

/// Whether Tables can write `path` without changing what the file claims to
/// be. `false` means a save to this path must be refused in favour of
/// Save As — never performed and hoped for.
pub fn is_writable_format(path: &str) -> bool {
    let extension = extension_of(path);
    WRITABLE_EXTENSIONS.iter().any(|writable| *writable == extension)
}

/// File name to offer in a Save As dialog for a document currently at `path`,
/// with the extension replaced by `.xlsx`.
///
/// Returns a bare file name, not a path — it is meant for a save dialog's
/// initial name, and the directory is the dialog's own business. A path with
/// no usable file name yields `Untitled.xlsx`.
///
/// Only the final extension is replaced, so `data.2026.csv` becomes
/// `data.2026.xlsx` rather than collapsing to `data.xlsx`.
pub fn xlsx_save_as_name(path: &str) -> String {
    let file_name = std::path::Path::new(path).file_name().and_then(|name| name.to_str());
    match file_name {
        None | Some("") => "Untitled.xlsx".to_string(),
        Some(name) => match name.rsplit_once('.') {
            // A leading dot is part of the name (".config"), not an extension
            // separator, so an empty stem keeps the whole name.
            Some((stem, _)) if !stem.is_empty() => format!("{stem}.xlsx"),
            _ => format!("{name}.xlsx"),
        },
    }
}

/// Lower-cased final extension of `path`, or `""` when it has none.
fn extension_of(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlsx_is_the_only_writable_format() {
        assert!(is_writable_format("book.xlsx"));
        for read_only in [
            "book.ods", "book.xls", "data.csv", "data.tsv", "macros.xlsm", "binary.xlsb",
        ] {
            assert!(
                !is_writable_format(read_only),
                "{read_only} must not be treated as writable — Tables has no writer for it"
            );
        }
    }

    /// Extension matching follows the loader's, which is case-insensitive:
    /// a capitalised name must not slip past the gate.
    #[test]
    fn writable_check_ignores_extension_case() {
        assert!(is_writable_format("BOOK.XLSX"));
        assert!(!is_writable_format("BOOK.ODS"));
    }

    /// A file with no extension is not writable: we would be guessing at the
    /// user's intent, and guessing wrong overwrites their file.
    #[test]
    fn extensionless_path_is_not_writable() {
        assert!(!is_writable_format("spreadsheet"));
        assert!(!is_writable_format(""));
    }

    #[test]
    fn save_as_name_replaces_the_extension() {
        assert_eq!(xlsx_save_as_name("report.ods"), "report.xlsx");
        assert_eq!(xlsx_save_as_name("data.csv"), "data.xlsx");
        assert_eq!(xlsx_save_as_name("/home/u/docs/budget.xls"), "budget.xlsx");
    }

    /// Only the final extension is replaced — a dotted stem survives intact.
    #[test]
    fn save_as_name_keeps_dotted_stems() {
        assert_eq!(xlsx_save_as_name("data.2026.csv"), "data.2026.xlsx");
        assert_eq!(xlsx_save_as_name("q1.q2.ods"), "q1.q2.xlsx");
    }

    #[test]
    fn save_as_name_handles_missing_and_leading_dot_extensions() {
        assert_eq!(xlsx_save_as_name("spreadsheet"), "spreadsheet.xlsx");
        assert_eq!(xlsx_save_as_name(".hidden"), ".hidden.xlsx");
    }

    #[test]
    fn save_as_name_falls_back_when_there_is_no_file_name() {
        assert_eq!(xlsx_save_as_name(""), "Untitled.xlsx");
        assert_eq!(xlsx_save_as_name("/"), "Untitled.xlsx");
    }

    /// The suggested name must itself be writable, or the dialog would offer
    /// a target the save path then refuses.
    #[test]
    fn suggested_name_is_always_writable() {
        for original in ["report.ods", "data.csv", "book.xls", "spreadsheet", ""] {
            let suggested = xlsx_save_as_name(original);
            assert!(
                is_writable_format(&suggested),
                "suggested {suggested:?} for {original:?} is not writable"
            );
        }
    }
}
