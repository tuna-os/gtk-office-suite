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
