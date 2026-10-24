//! Spec 4.2 version rule 3: a writer MUST NOT publish a file it has not opened
//! with the format's own reader (a ZIP archive whose package structure parses
//! for OOXML and ODF). A WOPI host receives the bytes from a client it does not
//! control, so it checks before it publishes.

use std::io::Cursor;

pub const ODT: &str = "application/vnd.oasis.opendocument.text";
pub const ODS: &str = "application/vnd.oasis.opendocument.spreadsheet";
pub const ODP: &str = "application/vnd.oasis.opendocument.presentation";
pub const ODG: &str = "application/vnd.oasis.opendocument.graphics";
pub const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
pub const PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";

/// File extension (without the dot) a WOPI client needs in `BaseFileName` to
/// choose an application. `None` for a MIME type this spike does not know.
pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    Some(match mime {
        ODT => "odt",
        ODS => "ods",
        ODP => "odp",
        ODG => "odg",
        DOCX => "docx",
        XLSX => "xlsx",
        PPTX => "pptx",
        _ => return None,
    })
}

enum Kind {
    Odf,
    Ooxml,
}

fn kind(mime: &str) -> Option<Kind> {
    match mime {
        ODT | ODS | ODP | ODG => Some(Kind::Odf),
        DOCX | XLSX | PPTX => Some(Kind::Ooxml),
        _ => None,
    }
}

/// `Ok` when `bytes` is a package of the right family. A MIME type that is not
/// an OOXML or ODF type is not checked (this host has no reader for it).
pub fn validate(mime: &str, bytes: &[u8]) -> Result<(), String> {
    let Some(kind) = kind(mime) else { return Ok(()) };
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("not a readable ZIP package: {e}"))?;
    let required = match kind {
        Kind::Odf => "content.xml",
        Kind::Ooxml => "[Content_Types].xml",
    };
    // `by_name` reads the local header and checks it against the central
    // directory, so a truncated member fails here too.
    let mut member = zip
        .by_name(required)
        .map_err(|e| format!("package has no readable {required}: {e}"))?;
    std::io::copy(&mut member, &mut std::io::sink())
        .map_err(|e| format!("{required} does not decompress: {e}"))?;
    Ok(())
}
