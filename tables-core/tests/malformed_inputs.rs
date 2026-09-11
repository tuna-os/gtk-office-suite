// malformed_inputs.rs — what the xlsx readers do with input built to break
// them, asserted rather than assumed.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Companion to the ODF harnesses in `letters-core` and `decks-core`.
// `docs/readiness-2026-09/interoperability.md` asks for malformed and
// truncated corpus cases with minimized failures retained; this is the
// deterministic PR-lane half, and `fuzz/` is the nightly libFuzzer half.
//
// Four readers are covered, and three of them are the reason this file
// exists: `read_charts_from_xlsx`, `read_cond_rules_from_xlsx` and
// `read_sheet_props_from_xlsx` return no `Result` at all. They are
// best-effort by design — a missing or unreadable part yields an empty
// result — which means **a panic is their only possible failure mode**, and
// nothing was checking for one.

use std::io::{Cursor, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use zip::write::SimpleFileOptions;

fn next(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545F491_4F6CDD1D)
}

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

// ── the parts a workbook is made of ──────────────────────────────────

const WORKBOOK: &str = "<workbook xmlns:r=\"r\"><sheets>\
    <sheet name=\"Sheet1\" sheetId=\"1\" r:id=\"rId1\"/></sheets>\
    <definedNames><definedName name=\"total\">Sheet1!$A$1:$A$5</definedName>\
    </definedNames></workbook>";

const WORKBOOK_RELS: &str = "<Relationships><Relationship Id=\"rId1\" \
    Target=\"worksheets/sheet1.xml\"/></Relationships>";

const SHEET: &str = "<worksheet><cols><col min=\"1\" max=\"3\" width=\"12\" hidden=\"1\"/>\
    </cols><sheetData><row r=\"1\"><c r=\"A1\" t=\"n\"><v>42</v></c>\
    <c r=\"B1\" t=\"str\"><f>SUM(A1:A5)</f><v>42</v></c></row></sheetData>\
    <conditionalFormatting sqref=\"A1:B9\"><cfRule type=\"cellIs\" \
    operator=\"greaterThan\" dxfId=\"0\" priority=\"1\"><formula>10</formula>\
    </cfRule></conditionalFormatting><pageSetup orientation=\"landscape\"/>\
    </worksheet>";

const STYLES: &str = "<styleSheet><dxfs count=\"1\"><dxf><fill><fgColor rgb=\"FFFF0000\"/>\
    </fill></dxf></dxfs></styleSheet>";

const DRAWING: &str = "<xdr:wsDr xmlns:xdr=\"x\"><xdr:twoCellAnchor><xdr:from>\
    <xdr:col>1</xdr:col><xdr:row>1</xdr:row></xdr:from></xdr:twoCellAnchor></xdr:wsDr>";

const CHART: &str = "<c:chartSpace xmlns:c=\"c\"><c:chart><c:plotArea><c:barChart>\
    <c:ser><c:val><c:numRef><c:f>Sheet1!$A$2:$A$5</c:f></c:numRef></c:val>\
    </c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>";

/// The parts, in the order a mutation seed picks between them.
fn parts() -> Vec<(&'static str, &'static str)> {
    vec![
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/worksheets/sheet1.xml", SHEET),
        ("xl/styles.xml", STYLES),
        ("xl/drawings/drawing1.xml", DRAWING),
        ("xl/charts/chart1.xml", CHART),
    ]
}

fn package(parts: &[(&str, String)]) -> Vec<u8> {
    let mut buffer = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut buffer));
        for (name, content) in parts {
            writer.start_file(*name, SimpleFileOptions::default()).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }
    buffer
}

fn valid_package() -> Vec<u8> {
    package(&parts().into_iter().map(|(n, c)| (n, c.to_string())).collect::<Vec<_>>())
}

/// Mutate exactly one part and repackage, so the zip layer always passes and
/// the reader's own XML handling is what gets exercised. Mutating the
/// packaged bytes instead would leave most seeds rejected by `zip` before
/// this crate ran at all.
fn package_with_one_part_mutated(seed: u64) -> (&'static str, Vec<u8>) {
    let all = parts();
    let mut s = seed | 1;
    let chosen = next(&mut s) as usize % all.len();
    let rebuilt: Vec<(&str, String)> = all
        .iter()
        .enumerate()
        .map(|(index, (name, content))| {
            if index == chosen {
                let bytes = mutate(seed, content.as_bytes());
                (*name, String::from_utf8_lossy(&bytes).to_string())
            } else {
                (*name, content.to_string())
            }
        })
        .collect();
    (all[chosen].0, package(&rebuilt))
}

/// Every reader that takes a path, run under `catch_unwind`.
///
/// `load_workbook` returns a `Result` and may legitimately fail. The other
/// three return no `Result`: for them, returning at all is success.
fn read_all_without_unwinding(bytes: &[u8]) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    let path = file.path().with_extension("xlsx");
    std::fs::copy(file.path(), &path).unwrap();
    let path = path.to_str().unwrap().to_string();
    let names = vec!["Sheet1".to_string()];

    let mut panicked = Vec::new();
    for (which, outcome) in [
        ("load_workbook", catch_unwind(AssertUnwindSafe(|| {
            let _ = tables_core::io::load_workbook(&path);
        }))),
        ("read_charts_from_xlsx", catch_unwind(AssertUnwindSafe(|| {
            tables_core::io::read_charts_from_xlsx(&path);
        }))),
        ("read_cond_rules_from_xlsx", catch_unwind(AssertUnwindSafe(|| {
            tables_core::io::read_cond_rules_from_xlsx(&path);
        }))),
        ("read_sheet_props_from_xlsx", catch_unwind(AssertUnwindSafe(|| {
            tables_core::io::read_sheet_props_from_xlsx(&path, &names);
        }))),
    ] {
        if outcome.is_err() {
            panicked.push(which);
        }
    }
    let _ = std::fs::remove_file(&path);

    if panicked.is_empty() { Ok(()) } else { Err(panicked.join(", ")) }
}

// ── structural hostility ─────────────────────────────────────────────

fn hostile_sheets() -> Vec<(String, String)> {
    let mut cases = vec![
        ("empty".to_string(), String::new()),
        ("not-xml".to_string(), "\u{0}\u{1}not xml at all".to_string()),
        ("truncated-mid-element".to_string(), "<worksheet><cols><col".to_string()),
        (
            "no-sheet-data".to_string(),
            "<worksheet><pageSetup orientation=\"portrait\"/></worksheet>".to_string(),
        ),
    ];

    // Column and row indices a reader might use to size or index a grid.
    // `min`/`max` of zero matter because xlsx columns are 1-based: a reader
    // that subtracts one underflows.
    for value in ["0", "-1", "999999999999999999999", "NaN", "4294967296", "1e400"] {
        cases.push((
            format!("col-range-{value}"),
            format!(
                "<worksheet><cols><col min=\"{value}\" max=\"{value}\" hidden=\"1\"/></cols>\
                 <sheetData/></worksheet>"
            ),
        ));
        cases.push((
            format!("hidden-row-{value}"),
            format!(
                "<worksheet><sheetData><row r=\"{value}\" hidden=\"1\"/></sheetData>\
                 </worksheet>"
            ),
        ));
        cases.push((
            format!("conditional-range-{value}"),
            format!(
                "<worksheet><sheetData/><conditionalFormatting sqref=\"A{value}:B{value}\">\
                 <cfRule type=\"cellIs\" operator=\"greaterThan\" dxfId=\"{value}\">\
                 <formula>{value}</formula></cfRule></conditionalFormatting></worksheet>"
            ),
        ));
    }

    // A hidden-column range spanning the whole 32-bit space. Not a crash
    // but a hang: the reader used to loop over every value in it, inserting
    // each into a set. A refusal is the only correct outcome.
    cases.push((
        "hidden-column-range-spanning-four-billion".into(),
        "<worksheet><cols><col min=\"1\" max=\"4294967295\" hidden=\"1\"/></cols>\
         <sheetData/></worksheet>"
            .into(),
    ));

    // A dxf index past the end of styles.xml — an out-of-bounds lookup if
    // the reader indexes the vector it built instead of `get`-ing it.
    cases.push((
        "dxf-index-past-the-end".into(),
        "<worksheet><sheetData/><conditionalFormatting sqref=\"A1\">\
         <cfRule type=\"cellIs\" operator=\"equal\" dxfId=\"4294967295\">\
         <formula>1</formula></cfRule></conditionalFormatting></worksheet>"
            .into(),
    ));

    // Bounded nesting: an unbounded version would abort the process on a
    // stack overflow rather than unwind, which only the out-of-process
    // libFuzzer lane can observe.
    let mut deep = String::from("<worksheet>");
    deep.push_str(&"<a>".repeat(10_000));
    deep.push_str("</worksheet>");
    cases.push(("deeply-nested".into(), deep));

    let many = "<conditionalFormatting sqref=\"A1\"><cfRule type=\"cellIs\" \
                operator=\"equal\" dxfId=\"0\"><formula>1</formula></cfRule>\
                </conditionalFormatting>"
        .repeat(2_000);
    cases.push((
        "two-thousand-conditional-rules".into(),
        format!("<worksheet><sheetData/>{many}</worksheet>"),
    ));

    cases
}

fn package_with_sheet(sheet: &str) -> Vec<u8> {
    let rebuilt: Vec<(&str, String)> = parts()
        .into_iter()
        .map(|(name, content)| {
            if name == "xl/worksheets/sheet1.xml" {
                (name, sheet.to_string())
            } else {
                (name, content.to_string())
            }
        })
        .collect();
    package(&rebuilt)
}

// ── tests ────────────────────────────────────────────────────────────

#[test]
fn a_hostile_sheet_does_not_crash_any_reader() {
    let mut panicked = Vec::new();
    for (name, sheet) in hostile_sheets() {
        if let Err(which) = read_all_without_unwinding(&package_with_sheet(&sheet)) {
            panicked.push(format!("{name} → {which}"));
        }
    }
    assert!(panicked.is_empty(), "panics: {panicked:?}");
}

const FIXED_SEEDS: u64 = 1_000;

#[test]
fn a_mutated_part_does_not_crash_any_reader() {
    let mut panicked = Vec::new();
    for seed in 1..=FIXED_SEEDS {
        let (part, bytes) = package_with_one_part_mutated(seed);
        if let Err(which) = read_all_without_unwinding(&bytes) {
            panicked.push(format!("seed {seed} ({part}) → {which}"));
            if panicked.len() > 4 {
                break;
            }
        }
    }
    assert!(panicked.is_empty(), "panics: {panicked:?}");
}

#[test]
fn a_mutated_package_does_not_crash_any_reader() {
    let base = valid_package();
    let mut panicked = Vec::new();
    for seed in 1..=FIXED_SEEDS {
        if let Err(which) = read_all_without_unwinding(&mutate(seed, &base)) {
            panicked.push(format!("seed {seed} → {which}"));
            if panicked.len() > 4 {
                break;
            }
        }
    }
    assert!(panicked.is_empty(), "panics: {panicked:?}");
}

/// The "retain minimized fuzz failures" mechanism — see
/// `tests/crashes/README.md`.
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
            if let Err(which) = read_all_without_unwinding(&bytes) {
                panicked.push(format!("{} → {which}", path.file_name().unwrap().to_string_lossy()));
            }
        }
    }

    assert!(panicked.is_empty(), "retained crash inputs still panic: {panicked:?}");
    println!("replayed {replayed} retained crash input(s) from {}", dir.display());
}

#[test]
#[ignore = "campaign-scale; run from the nightly stress workflow"]
fn seed_campaign() {
    let base = valid_package();
    let from: u64 = std::env::var("MALFORMED_SEED_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let count: u64 = std::env::var("MALFORMED_SEED_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);

    let mut panicked = Vec::new();
    for seed in from..from + count {
        let (part, mutated_part) = package_with_one_part_mutated(seed);
        if let Err(which) = read_all_without_unwinding(&mutated_part) {
            panicked.push(format!("part/{seed} ({part}) → {which}"));
        }
        if let Err(which) = read_all_without_unwinding(&mutate(seed, &base)) {
            panicked.push(format!("package/{seed} → {which}"));
        }
        if panicked.len() > 9 {
            break;
        }
    }
    assert!(
        panicked.is_empty(),
        "{panicked:?} — minimize one and commit it to tests/crashes/"
    );
    println!("{count} seeds × 2 mutation modes from seed {from}: no panics");
}
