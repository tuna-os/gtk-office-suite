// malformed_inputs.rs — what the ODT reader does with input built to break
// it, asserted rather than assumed.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// `docs/readiness-2026-09/interoperability.md` asks for "malformed/truncated
// corpus cases" and for minimized fuzz failures to be retained. This is the
// PR-lane half: deterministic, fast, and seeded, so a finding is a named
// seed rather than a rumour. The nightly libFuzzer half lives in `fuzz/`.
//
// These currently pass, and that is the point: the robustness was real but
// nothing asserted it, so nothing would notice it going away. A reader that
// starts panicking on a truncated file fails here, on the pull request that
// did it.
//
// The contract under test is narrow and deliberately not "it parses":
//
//   * No panic. A reader returning Err is a correct reader; a reader
//     unwinding is a crash the GUI cannot recover from.
//   * No hang. Each case is bounded, and a case that stops finishing shows
//     up as a CI timeout rather than as a green run.
//   * Nothing written outside the input. Covered by `hostile_packages.rs`.

use std::io::{Cursor, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use zip::write::SimpleFileOptions;

// ── seeded mutation ──────────────────────────────────────────────────
// The same explicit xorshift64* the stateful harnesses use, for the same
// reason: a seed must replay identically on any machine and any version of
// any dependency, which `rand` does not promise.

fn next(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545F491_4F6CDD1D)
}

/// Apply between one and eight byte-level edits to `base`.
///
/// Byte mutation of a ZIP is low-yield on its own — most edits break a CRC
/// or a header and the zip layer rejects the file before a parser sees it
/// — but it is the cheapest way to reach the reject paths themselves, which
/// is where an unwrap on a malformed header would live.
fn mutate(seed: u64, base: &[u8]) -> Vec<u8> {
    let mut s = seed | 1;
    let mut out = base.to_vec();
    for _ in 0..(next(&mut s) % 8) + 1 {
        if out.is_empty() {
            break;
        }
        let at = next(&mut s) as usize % out.len();
        match next(&mut s) % 4 {
            0 => out[at] = (next(&mut s) & 0xff) as u8,
            1 => out.truncate(at),
            2 => out.insert(at, (next(&mut s) & 0xff) as u8),
            _ => {
                let n = (next(&mut s) as usize % 32).min(out.len() - at);
                out[at..at + n].fill(0);
            }
        }
    }
    out
}

fn valid_odt() -> Vec<u8> {
    let doc = letters_core::model::Document::from_plain_text(
        "A letter with some text in it, long enough to have structure.",
    );
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().with_extension("odt");
    letters_core::odt::write(&doc, path.to_str().unwrap()).expect("write an odt");
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    bytes
}

/// Mutate the document XML and repackage it into a *valid* archive.
///
/// Mutating the packaged bytes directly leaves three quarters of the seeds
/// dying at the zip layer (measured: 503 of 2,000 produced a readable
/// archive), which tests the zip crate more than it tests this reader.
/// Repackaging guarantees every seed reaches the XML parser, which is where
/// this crate's own code is.
fn mutate_inside_package(seed: u64, xml: &str) -> Vec<u8> {
    let mutated = mutate(seed, xml.as_bytes());
    package(&String::from_utf8_lossy(&mutated))
}

fn read_without_unwinding(bytes: &[u8]) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    let path = file.path().to_str().unwrap().to_string();
    catch_unwind(AssertUnwindSafe(|| {
        let _ = letters_core::odt::read(&path);
    }))
    .map_err(|_| "reader panicked".to_string())
}

// ── structural hostility ─────────────────────────────────────────────
// A valid archive whose XML is the problem. This is the axis that matters:
// byte mutation rarely produces a well-formed package, so without these the
// XML paths are barely reached at all.

fn package(content: &str) -> Vec<u8> {
    let mut buffer = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut buffer));
        writer.start_file("content.xml", SimpleFileOptions::default()).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    buffer
}

/// Each case names a way a document can be hostile rather than merely
/// broken. The depths and counts are bounded on purpose: an unbounded
/// version would be testing the machine, and a stack overflow aborts the
/// process rather than unwinding, so it cannot be caught here — it needs
/// the out-of-process libFuzzer lane.
fn hostile_documents() -> Vec<(String, String)> {
    let mut cases = Vec::new();

    for depth in [100usize, 1_000, 10_000] {
        let mut xml = String::from(
            "<office:document-content xmlns:office=\"o\" xmlns:text=\"t\">\
             <office:body><office:text>",
        );
        xml.push_str(&"<text:span>".repeat(depth));
        xml.push('x');
        xml.push_str(&"</text:span>".repeat(depth));
        xml.push_str("</office:text></office:body></office:document-content>");
        cases.push((format!("nested-spans-{depth}"), xml));
    }

    cases.push((
        "truncated-mid-element".into(),
        "<office:document-content><office:body><office:text><text:p>a".into(),
    ));
    cases.push((
        "truncated-mid-attribute".into(),
        "<office:document-content><office:body><office:text><text:p style=\"".into(),
    ));
    cases.push(("empty".into(), String::new()));
    cases.push(("not-xml-at-all".into(), "\u{0}\u{1}\u{2}not xml".into()));

    // Entity expansion. quick-xml does not expand DTD entities, so this is
    // a regression guard on that property rather than a live hazard: swap
    // in a parser that does and this case is how you find out.
    cases.push((
        "billion-laughs".into(),
        "<!DOCTYPE d [<!ENTITY a \"aaaaaaaaaa\">\
         <!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;\">\
         <!ENTITY c \"&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;\">\
         <!ENTITY d \"&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;\">]>\
         <office:document-content><office:body><office:text>\
         <text:p>&d;</text:p></office:text></office:body></office:document-content>"
            .into(),
    ));

    // Numbers a reader might parse, index with, or allocate from.
    for value in ["999999999999999999999", "-1", "NaN", "1e400", "18446744073709551616", "0"] {
        cases.push((
            format!("outline-level-{value}"),
            format!(
                "<office:document-content xmlns:office=\"o\" xmlns:text=\"t\">\
                 <office:body><office:text>\
                 <text:h text:outline-level=\"{value}\">heading</text:h>\
                 </office:text></office:body></office:document-content>"
            ),
        ));
    }

    let attributes: String = (0..20_000).map(|i| format!(" a{i}=\"v\"")).collect();
    cases.push((
        "twenty-thousand-attributes".into(),
        format!(
            "<office:document-content{attributes}><office:body><office:text>\
             <text:p>x</text:p></office:text></office:body></office:document-content>"
        ),
    ));

    // Cells all claiming the same coordinates — the shape of #532, arriving
    // from a file rather than from an editing command.
    let cells = "<table:table-cell><text:p>x</text:p></table:table-cell>".repeat(500);
    cases.push((
        "five-hundred-cells-in-one-row".into(),
        format!(
            "<office:document-content xmlns:office=\"o\" xmlns:text=\"t\" xmlns:table=\"tb\">\
             <office:body><office:text><table:table><table:table-row>{cells}\
             </table:table-row></table:table></office:text></office:body>\
             </office:document-content>"
        ),
    ));

    cases
}

// ── tests ────────────────────────────────────────────────────────────

#[test]
fn a_hostile_document_is_an_error_not_a_crash() {
    let mut panicked = Vec::new();
    for (name, content) in hostile_documents() {
        if read_without_unwinding(&package(&content)).is_err() {
            panicked.push(name);
        }
    }
    assert!(panicked.is_empty(), "the reader panicked on: {panicked:?}");
}

/// Fixed seeds run on every pull request. A seed that finds something is
/// added here, which is how a find becomes permanent.
const FIXED_SEEDS: u64 = 2_000;

/// A content.xml exercising the constructs the reader actually branches on,
/// so mutating it reaches styles, headings, lists, tables and spans rather
/// than only plain paragraphs.
const VALID_CONTENT_XML: &str = "<office:document-content xmlns:office=\"o\" \
    xmlns:text=\"t\" xmlns:table=\"tb\" xmlns:style=\"s\" xmlns:fo=\"f\">\
    <office:automatic-styles>\
    <style:style style:name=\"P1\" style:family=\"paragraph\">\
    <style:paragraph-properties fo:text-align=\"center\"/>\
    <style:text-properties fo:font-weight=\"bold\" fo:font-size=\"14pt\"/>\
    </style:style></office:automatic-styles>\
    <office:body><office:text>\
    <text:h text:outline-level=\"2\">A heading</text:h>\
    <text:p text:style-name=\"P1\">Styled <text:span>nested run</text:span> text</text:p>\
    <text:list><text:list-item><text:p>item</text:p></text:list-item></text:list>\
    <table:table><table:table-row>\
    <table:table-cell><text:p>r0c0</text:p></table:table-cell>\
    <table:table-cell><text:p>r0c1</text:p></table:table-cell>\
    </table:table-row></table:table>\
    <text:p>trailing paragraph</text:p>\
    </office:text></office:body></office:document-content>";

/// Mutations of the packaged bytes. Most are rejected by the zip layer,
/// which is itself worth covering: that rejection path is where a panic on
/// a malformed header would live.
#[test]
fn mutated_packages_are_errors_not_crashes() {
    let base = valid_odt();
    let mut panicked = Vec::new();
    for seed in 1..=FIXED_SEEDS {
        if read_without_unwinding(&mutate(seed, &base)).is_err() {
            panicked.push(seed);
            if panicked.len() > 4 {
                break;
            }
        }
    }
    assert!(panicked.is_empty(), "the reader panicked on seeds: {panicked:?}");
}

/// Mutations of the document XML, repackaged so every seed reaches the
/// parser. This is the half that exercises this crate rather than `zip`.
#[test]
fn mutated_document_xml_is_an_error_not_a_crash() {
    let xml = VALID_CONTENT_XML;
    let mut panicked = Vec::new();
    for seed in 1..=FIXED_SEEDS {
        if read_without_unwinding(&mutate_inside_package(seed, xml)).is_err() {
            panicked.push(seed);
            if panicked.len() > 4 {
                break;
            }
        }
    }
    assert!(panicked.is_empty(), "the reader panicked on seeds: {panicked:?}");
}

/// Every input under `tests/crashes/` is replayed.
///
/// This is the "retain minimized fuzz failures" mechanism: when the nightly
/// libFuzzer lane or a campaign seed finds a crash, the minimized input is
/// committed there and this test is what keeps it fixed. An empty directory
/// is a true statement about the present, not a gap — and the test says so
/// rather than passing silently, so nobody mistakes "no files" for "not
/// wired up".
#[test]
fn every_retained_crash_input_still_reads_without_panicking() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/crashes");
    let mut replayed = 0;
    let mut panicked = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") || path.is_dir() {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            replayed += 1;
            if read_without_unwinding(&bytes).is_err() {
                panicked.push(path.file_name().unwrap().to_string_lossy().to_string());
            }
        }
    }

    assert!(panicked.is_empty(), "retained crash inputs still panic: {panicked:?}");
    println!("replayed {replayed} retained crash input(s) from {}", dir.display());
}

/// The campaign-scale version, for the nightly. Ignored in the PR lane for
/// the same reason the stateful campaigns are: it answers "how often", not
/// "does this change break it".
#[test]
#[ignore = "campaign-scale; run from the nightly stress workflow"]
fn seed_campaign() {
    let base = valid_odt();
    let from: u64 = std::env::var("MALFORMED_SEED_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let count: u64 = std::env::var("MALFORMED_SEED_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);

    let mut panicked = Vec::new();
    for seed in from..from + count {
        for (mode, bytes) in [
            ("package", mutate(seed, &base)),
            ("xml", mutate_inside_package(seed, VALID_CONTENT_XML)),
        ] {
            if read_without_unwinding(&bytes).is_err() {
                panicked.push(format!("{mode}/{seed}"));
            }
        }
        if panicked.len() > 9 {
            break;
        }
    }
    assert!(
        panicked.is_empty(),
        "{panicked:?} panicked — minimize one and commit it to tests/crashes/"
    );
    println!("{count} seeds × 2 mutation modes from seed {from}: no panics");
}
