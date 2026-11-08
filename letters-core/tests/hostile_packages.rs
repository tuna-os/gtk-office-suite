// hostile_packages.rs — what the ODT reader does with a package built to
// hurt it.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Companion to `decks-core/tests/hostile_packages.rs`. The bound lives in
// `suite_common_core::zip_guard`; these prove this crate's reader is
// actually behind it (#442).

use std::io::{Cursor, Write};
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;

/// Just past `ZipLimits::default().max_entry_bytes`.
const OVERSIZED: usize = 136 * 1024 * 1024;

fn package(parts: &[(&str, usize)], literal: &[(&str, &str)]) -> NamedTempFile {
    let mut buffer = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in literal {
            writer.start_file(*name, options).unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        for (name, size) in parts {
            writer.start_file(*name, options).unwrap();
            let chunk = vec![b'A'; 1024 * 1024];
            let mut left = *size;
            while left > 0 {
                let take = left.min(chunk.len());
                writer.write_all(&chunk[..take]).unwrap();
                left -= take;
            }
        }
        writer.finish().unwrap();
    }
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(&buffer).unwrap();
    file.flush().unwrap();
    file
}

/// Before the bound this allocated until the process died. The error must
/// name the limit rather than claim the file is not an ODT: content.xml is
/// present, and saying otherwise sends the user after the wrong problem.
#[test]
fn an_odt_whose_content_expands_past_the_budget_is_refused_not_allocated() {
    let file = package(&[("content.xml", OVERSIZED)], &[]);
    let err = letters_core::odt::read(file.path().to_str().unwrap())
        .expect_err("an oversized content.xml must not be read");
    assert!(
        err.contains("package member") || err.contains("decompress"),
        "the error must name the limit it hit, got: {err}"
    );
}

/// DOCX preflight must budget binary members as well as XML. Embedded media
/// and objects are attacker-controlled package members too, and the parser
/// must never get a chance to decompress one past the shared limit.
#[test]
fn a_docx_whose_binary_member_expands_past_the_budget_is_refused() {
    let file = package(
        &[("word/media/bomb.bin", OVERSIZED)],
        &[("word/document.xml", "<w:document xmlns:w=\"w\"><w:body/></w:document>")],
    );
    let err = letters_core::docx::read(file.path().to_str().unwrap())
        .expect_err("an oversized binary DOCX member must not be read");
    assert!(
        err.contains("package member") || err.contains("decompress"),
        "the error must name the limit it hit, got: {err}"
    );
}

/// styles.xml is optional, so an oversized one must not fail the document
/// — but it must not be read either. The document opens with default
/// styling, which is what a missing styles.xml already did.
#[test]
fn an_oversized_optional_part_is_skipped_rather_than_read_or_fatal() {
    let file = package(
        &[("styles.xml", OVERSIZED)],
        &[(
            "content.xml",
            "<office:document-content xmlns:office=\"o\" xmlns:text=\"t\">\
             <office:body><office:text><text:p>hello</text:p></office:text></office:body>\
             </office:document-content>",
        )],
    );
    let doc = letters_core::odt::read(file.path().to_str().unwrap())
        .expect("an oversized optional part must not fail the document");
    assert!(
        doc.to_plain_text().contains("hello"),
        "the content still parsed: {:?}",
        doc.to_plain_text()
    );
}

#[test]
fn a_package_with_an_implausible_member_count_is_refused_up_front() {
    let mut buffer = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options = SimpleFileOptions::default();
        writer.start_file("content.xml", options).unwrap();
        writer.write_all(b"<office:document-content/>").unwrap();
        for index in 0..20_000 {
            writer.start_file(format!("junk/{index}"), options).unwrap();
        }
        writer.finish().unwrap();
    }
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(&buffer).unwrap();
    file.flush().unwrap();

    let err = letters_core::odt::read(file.path().to_str().unwrap())
        .expect_err("20,000 members is not a document");
    assert!(err.contains("members"), "got: {err}");
}

/// Opaque capture reads every member the format reader did not claim,
/// which makes it the widest decompression surface in the suite — and it
/// keeps what it reads.
#[test]
fn opaque_capture_is_bounded_too() {
    let file = package(&[("extras/huge.bin", OVERSIZED)], &[("content.xml", "<x/>")]);
    let err = suite_common_core::interop::OpaquePackage::capture(file.path(), &["content.xml"])
        .expect_err("an oversized opaque part must not be captured");
    assert!(
        err.contains("package member") || err.contains("decompress"),
        "the error must name the limit it hit, got: {err}"
    );
}

/// The limits must not reject real documents.
#[test]
fn an_ordinary_document_still_opens() {
    let doc = letters_core::model::Document::from_plain_text("an ordinary letter");
    let file = NamedTempFile::new().unwrap();
    let path = file.path().with_extension("odt");
    letters_core::odt::write(&doc, path.to_str().unwrap()).expect("write an odt");

    let read_back = letters_core::odt::read(path.to_str().unwrap()).expect("it opens");
    assert!(read_back.to_plain_text().contains("an ordinary letter"));
    let _ = std::fs::remove_file(&path);
}

/// Letters can't decrypt a document, and said so with an error about ZIP
/// archives or UTF-8 that never mentions the password (#1655: the admitted
/// scope has no encryption, and a refusal has to say why). A password-
/// protected .docx is an OLE compound file, not a ZIP.
#[test]
fn a_password_protected_docx_is_refused_by_naming_the_password() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]).unwrap();
    file.write_all(&[0u8; 4096]).unwrap();
    file.flush().unwrap();
    let err = letters_core::docx::read(file.path().to_str().unwrap()).expect_err("an encrypted package must not open");
    assert!(err.contains("password"), "the error must name the password, got: {err}");
    assert!(!err.contains("EOCD"), "not the ZIP parser's complaint: {err}");
}

/// An ODT with encrypted parts is a ZIP whose content is ciphertext.
#[test]
fn an_encrypted_odt_is_refused_by_naming_the_password() {
    let manifest = "<manifest:manifest><manifest:file-entry manifest:full-path=\"content.xml\"><manifest:encryption-data/></manifest:file-entry></manifest:manifest>";
    let file = package(&[], &[("META-INF/manifest.xml", manifest), ("content.xml", "\u{1}\u{2}\u{3}")]);
    let err = letters_core::odt::read(file.path().to_str().unwrap()).expect_err("an encrypted package must not open");
    assert!(err.contains("password"), "the error must name the password, got: {err}");
    assert!(!err.contains("UTF-8"), "not the XML reader's complaint: {err}");
}
