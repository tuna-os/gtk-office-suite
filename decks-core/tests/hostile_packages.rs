// hostile_packages.rs — what the production readers do with a package
// built to hurt them.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The unit tests in `suite_common_core::zip_guard` prove the bound works.
// These prove the readers actually use it: a limit that exists in a helper
// no reader calls is the kind of hardening that reads well and protects
// nothing. Every archive here is built in memory, so what is being tested
// is visible in the test rather than committed as an opaque binary
// (#442, docs/readiness-2026-09/interoperability.md).

use std::io::{Cursor, Write};
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;

/// Just past `ZipLimits::default().max_entry_bytes`.
const OVERSIZED: usize = 136 * 1024 * 1024;

/// A member that decompresses to `size` bytes of one repeated byte. Highly
/// compressible, so the archive on disk stays tiny — which is the whole
/// trick: the cost is paid by the reader, not the sender.
fn package_with_oversized_part(parts: &[(&str, usize)], literal: &[(&str, &str)]) -> NamedTempFile {
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
            // Written in chunks so the test itself does not need the whole
            // thing resident.
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

/// 136 MiB of content.xml: past the 128 MiB a single package member is
/// allowed to expand to. The per-entry bound rather than the 512 MiB total
/// on purpose — it is the same guard on the same call path, and proving it
/// costs a quarter of the data and a quarter of the time in the PR lane.
/// The total-budget bound has its own unit test in `zip_guard`.
///
/// Before the bound this allocated until the process died; now it is an
/// error naming the limit it hit.
#[test]
fn an_odp_whose_content_expands_past_the_budget_is_refused_not_allocated() {
    let file = package_with_oversized_part(&[("content.xml", OVERSIZED)], &[]);
    let err = decks_core::odp::read(file.path().to_str().unwrap())
        .expect_err("an oversized content.xml must not be read");
    assert!(
        err.contains("decompress") || err.contains("package member"),
        "the error must name the limit it hit, got: {err}"
    );
}

#[test]
fn a_pptx_whose_presentation_expands_past_the_budget_is_refused_not_allocated() {
    let file = package_with_oversized_part(
        &[("ppt/presentation.xml", OVERSIZED)],
        &[("ppt/_rels/presentation.xml.rels", "<Relationships/>")],
    );
    let err = decks_core::engine::read_pptx(file.path().to_str().unwrap())
        .expect_err("an oversized presentation.xml must not be read");
    // Specifically the limit, not "missing ppt/presentation.xml" — the
    // part is right there. A reader that reported it as missing would send
    // the user looking for the wrong problem, and would also let this test
    // pass for the wrong reason.
    assert!(
        err.contains("package member") || err.contains("decompress"),
        "the error must name the limit it hit, got: {err}"
    );
}

/// An archive with an implausible number of members is refused before any
/// of them is read, so the cost of saying no does not scale with the
/// attack.
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

    let err = decks_core::odp::read(file.path().to_str().unwrap())
        .expect_err("20,000 members is not a presentation");
    assert!(err.contains("members"), "got: {err}");
}

/// The limits must not reject real files. An ordinary deck — well inside
/// every bound — still opens, which is the property a too-eager limit
/// would break and users, not this suite, would discover.
#[test]
fn an_ordinary_deck_still_opens() {
    let deck = decks_core::engine::Deck::default();
    let file = NamedTempFile::new().unwrap();
    let path = file.path().with_extension("pptx");
    decks_core::engine::write_pptx(path.to_str().unwrap(), &deck).expect("write a deck");

    decks_core::engine::read_pptx(path.to_str().unwrap()).expect("an ordinary deck opens");
    let _ = std::fs::remove_file(&path);
}
