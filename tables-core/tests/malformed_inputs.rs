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

// ── unbounded nesting, out of process ─────────────────────────────────
// A stack overflow aborts the process instead of unwinding, so the bounded
// "deeply-nested" case above stops at 10,000 levels. Here every reader runs
// in a child process — this test binary again, with `DEEP_NESTING_CHILD`
// set — on 100,000 levels, and the parent fails when the child dies (an
// overflow) or overruns its budget (super-linear work) (#1206).
// `DEEP_NESTING_DEPTH` raises the depth.

const DEEP_NESTING_CHILD: &str = "DEEP_NESTING_CHILD";

fn depth() -> usize {
    std::env::var("DEEP_NESTING_DEPTH").ok().and_then(|v| v.parse().ok()).unwrap_or(100_000)
}

/// Runs only as the child `deep_nesting_neither_overflows_nor_crawls` starts.
#[test]
#[ignore = "the child process of deep_nesting_neither_overflows_nor_crawls"]
fn deep_nesting_child() {
    if std::env::var_os(DEEP_NESTING_CHILD).is_none() {
        return;
    }
    let depth = depth();
    for (name, sheet) in [
        ("unknown elements", format!("<worksheet>{}x{}</worksheet>", "<a>".repeat(depth), "</a>".repeat(depth))),
        (
            "rich text",
            format!(
                "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData><row r=\"1\">\
                 <c r=\"A1\" t=\"inlineStr\"><is>{}<t>x</t>{}</is></c></row></sheetData></worksheet>",
                "<r>".repeat(depth),
                "</r>".repeat(depth)
            ),
        ),
    ] {
        eprintln!("reading xlsx {name}");
        let _ = read_all_without_unwinding(&package_with_sheet(&sheet));
    }
    eprintln!("reading ods tables");
    let content = format!(
        "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\">\
         <office:body><office:spreadsheet><table:table table:name=\"S\"><table:table-row><table:table-cell>{}<text:p>x</text:p>{}\
         </table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>",
        "<text:span>".repeat(depth),
        "</text:span>".repeat(depth)
    );
    let ods = package(&[
        ("mimetype", "application/vnd.oasis.opendocument.spreadsheet".to_string()),
        ("content.xml", content),
    ]);
    let mut file = tempfile::Builder::new().suffix(".ods").tempfile().unwrap();
    file.write_all(&ods).unwrap();
    file.flush().unwrap();
    let path = file.path().to_str().unwrap().to_string();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _ = tables_core::io::load_workbook(&path);
        let _ = tables_core::io::read_sheet_props_from_ods(&path);
    }));
    eprintln!("deep nesting: all read");
}

#[test]
fn deep_nesting_neither_overflows_nor_crawls() {
    run_child("deep_nesting_child", DEEP_NESTING_CHILD, "deep nesting", None, std::time::Duration::from_secs(120));
}

/// Start the test `test` in a process of its own, with `marker` set, and fail on its death or on its overrunning `budget`, naming the
/// input it had reached. With `memory_kib` the child may address no more
/// than that (`ulimit -v`), so an allocation that would take the machine
/// down ends the child instead.
fn run_child(test: &str, marker: &str, what: &str, memory_kib: Option<u64>, budget: std::time::Duration) {
    use std::io::Read;
    let exe = std::env::current_exe().unwrap();
    let mut command = match memory_kib {
        Some(kib) => {
            let mut shell = std::process::Command::new("sh");
            shell.args(["-c", &format!("ulimit -v {kib} && exec \"$0\" \"$@\"")]).arg(exe);
            shell
        }
        None => std::process::Command::new(exe),
    };
    let mut child = command
        .args([test, "--exact", "--include-ignored", "--nocapture", "--test-threads=1"])
        .env(marker, "1")
        .env_remove("RUST_MIN_STACK")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("start the child");
    let mut stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if started.elapsed() > budget {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    let text = reader.join().unwrap();
    let reached = text.lines().rfind(|l| l.starts_with("reading ")).unwrap_or("(nothing)");
    match status {
        None => panic!("{what} took over {budget:?}; it was {reached}"),
        Some(status) => {
            assert!(status.success(), "a reader died on {what} ({status}) {reached}\n{text}");
            assert!(text.contains(&format!("{what}: all read")), "the child stopped early:\n{text}");
        }
    }
}

// ── counts and ranges that multiply, out of process ───────────────────
// A file states a count or a range in a few bytes, and a reader that takes
// it at its word allocates or loops for it. None of these panics: the
// process runs out of memory or never comes back, which `catch_unwind`
// cannot see and which, in process, would take the test run down with it.
// So they run in a child that may address 4 GiB, and the parent fails when
// the child dies, overruns its budget, or reads something it should not.
// Each file is a few kilobytes. On the code before this section was added:
// the two .ods files ended the process on open (one styled cell repeated
// 4096 times in a row repeated 4096 times is 16.7 million styles), the
// chart anchor never came back, and the merge and the print area opened
// and then ended the process on the first save or PDF export.

const SIZE_BOMBS_CHILD: &str = "SIZE_BOMBS_CHILD";

fn temp_file(bytes: &[u8], suffix: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    file
}

/// An .ods whose one sheet, `S`, holds a cell and then `rows`.
fn ods_with_rows(rows: &str) -> Vec<u8> {
    let content = format!(
        "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\">\
         <office:automatic-styles>\
         <style:style style:name=\"ro1\" style:family=\"table-row\"><style:table-row-properties style:row-height=\"1cm\"/></style:style>\
         <style:style style:name=\"ce1\" style:family=\"table-cell\"><style:text-properties fo:font-weight=\"bold\"/></style:style>\
         </office:automatic-styles>\
         <office:body><office:spreadsheet><table:table table:name=\"S\"><table:table-row>\
         <table:table-cell office:value-type=\"string\"><text:p>x</text:p></table:table-cell></table:table-row>{rows}\
         </table:table></office:spreadsheet></office:body></office:document-content>"
    );
    let manifest = "<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\">\
                    <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.spreadsheet\"/>\
                    <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>";
    package(&[
        ("mimetype", "application/vnd.oasis.opendocument.spreadsheet".to_string()),
        ("META-INF/manifest.xml", manifest.to_string()),
        ("content.xml", content),
    ])
}

/// A workbook whose sheet holds one value and then `sheet_extra`, with
/// `names` in its `definedNames` and `more` parts beside the sheet.
fn xlsx_with(sheet_extra: &str, names: &str, more: &[(&str, &str)]) -> Vec<u8> {
    let workbook = format!(
        "<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets>\
         <sheet name=\"Sheet1\" sheetId=\"1\" r:id=\"rId1\"/></sheets><definedNames>{names}</definedNames></workbook>"
    );
    let sheet = format!(
        "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" \
         xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheetData><row r=\"1\">\
         <c r=\"A1\"><v>1</v></c></row></sheetData>{sheet_extra}</worksheet>"
    );
    let mut parts = vec![
        (
            "_rels/.rels",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" \
             Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" \
             Target=\"xl/workbook.xml\"/></Relationships>"
                .to_string(),
        ),
        ("xl/workbook.xml", workbook),
        (
            "xl/_rels/workbook.xml.rels",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" \
             Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" \
             Target=\"worksheets/sheet1.xml\"/></Relationships>"
                .to_string(),
        ),
        ("xl/worksheets/sheet1.xml", sheet),
    ];
    parts.extend(more.iter().map(|(name, content)| (*name, content.to_string())));
    package(&parts)
}

/// Does its work only as the child
/// `a_count_or_range_that_multiplies_stays_in_the_sheet` starts, with
/// `SIZE_BOMBS_CHILD` set; run with the other tests it returns at once.
#[test]
fn size_bombs_child() {
    use tables_core::sheet::{MAX_SHEET_CELLS, SHEET_MAX_ROWS};
    if std::env::var_os(SIZE_BOMBS_CHILD).is_none() {
        return;
    }
    let inside = |sheet: &tables_core::sheet::SheetModel, (top, left, bottom, right): (usize, usize, usize, usize)| {
        top <= bottom && left <= right && bottom < sheet.rows && right < sheet.cols
    };

    // 20,000 times 4096 rows with a height: 82 million rows, on a grid of
    // one million.
    eprintln!("reading ods rows repeated past the grid");
    let rows = "<table:table-row table:style-name=\"ro1\" table:number-rows-repeated=\"4096\"><table:table-cell/></table:table-row>";
    let file = temp_file(&ods_with_rows(&rows.repeat(20_000)), ".ods");
    let path = file.path().to_str().unwrap();
    let (_, sheets) = tables_core::io::load_workbook(path).expect("the sheet opens");
    assert_eq!((sheets[0].rows, sheets[0].cols), (100, 26), "at the size of its one cell");
    let props = tables_core::io::read_sheet_props_from_ods(path);
    let heights = &props["S"].row_heights;
    assert!(heights.len() <= SHEET_MAX_ROWS && heights.keys().all(|&r| r < SHEET_MAX_ROWS), "{} heights", heights.len());

    // A styled cell repeated 4096 times in a row repeated 4096 times, 64
    // times over. `office:value-type=` is in the tag, which is all the
    // layout reader asks before it styles a repeated cell, and not where
    // the value reader finds a value: so the sheet stays one cell large.
    eprintln!("reading ods styled cells repeated in repeated rows");
    let cells = "<table:table-row table:number-rows-repeated=\"4096\"><table:table-cell table:style-name=\"ce1\" \
                 table:content-validation-name=\"office:value-type=\" table:number-columns-repeated=\"4096\"/></table:table-row>";
    let file = temp_file(&ods_with_rows(&cells.repeat(64)), ".ods");
    let path = file.path().to_str().unwrap();
    let (_, sheets) = tables_core::io::load_workbook(path).expect("the sheet opens");
    assert_eq!((sheets[0].rows, sheets[0].cols), (100, 26), "at the size of its one cell");
    let styled = tables_core::io::read_sheet_props_from_ods(path)["S"].cell_styles.len();
    assert!(styled <= MAX_SHEET_CELLS, "{styled} styles for a sheet that opens with at most {MAX_SHEET_CELLS} cells");

    // A merge of the whole grid: the writer visits every cell it covers.
    eprintln!("reading xlsx merge of the whole grid");
    let file = temp_file(&xlsx_with("<mergeCells count=\"1\"><mergeCell ref=\"A1:XFD1048576\"/></mergeCells>", "", &[]), ".xlsx");
    let (_, sheets) = tables_core::io::load_workbook(file.path().to_str().unwrap()).expect("the workbook opens");
    let sheet = &sheets[0];
    assert_eq!(sheet.merges, [(0, 0, sheet.rows, sheet.cols)], "the merge, as much of it as the sheet holds");
    assert_eq!(sheet.used_extent(), Some((sheet.rows - 1, sheet.cols - 1)));
    tables_core::io::save_sheets_to_xlsx_bytes(&sheets, None).expect("and saves");

    // A print area of the whole grid: the PDF export writes every cell of it.
    eprintln!("reading xlsx print area of the whole grid");
    let names = "<definedName name=\"_xlnm.Print_Area\" localSheetId=\"0\">Sheet1!$A$1:$XFD$1048576</definedName>";
    let file = temp_file(&xlsx_with("", names, &[]), ".xlsx");
    let (engine, sheets) = tables_core::io::load_workbook(file.path().to_str().unwrap()).expect("the workbook opens");
    let area = sheets[0].print_area.expect("the print area");
    assert!(inside(&sheets[0], area), "{area:?} on a sheet of {} by {}", sheets[0].rows, sheets[0].cols);
    let typst = tables_core::export::to_typst_for_sheet(&engine, &sheets[0]);
    assert!(typst.len() < 1 << 20, "{} bytes of Typst for a sheet of one value", typst.len());

    // A chart from row 1 to row 1e19: its height is a sum over the rows.
    eprintln!("reading xlsx chart anchored past the grid");
    let rels = |kind: &str, target: &str| {
        format!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" \
             Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{target}\"/></Relationships>"
        )
    };
    let drawing = "<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" \
                   xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
                   xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" \
                   xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><xdr:twoCellAnchor>\
                   <xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>\
                   <xdr:to><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>10000000000000000000</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>\
                   <xdr:graphicFrame><a:graphic><a:graphicData><c:chart r:id=\"rId1\"/></a:graphicData></a:graphic></xdr:graphicFrame>\
                   </xdr:twoCellAnchor></xdr:wsDr>";
    let chart = "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart><c:plotArea><c:barChart>\
                 <c:ser><c:val><c:numRef><c:f>Sheet1!$A$1:$A$1</c:f></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>";
    let workbook = xlsx_with(
        "<drawing r:id=\"rId1\"/>",
        "",
        &[
            ("xl/worksheets/_rels/sheet1.xml.rels", &rels("drawing", "../drawings/drawing1.xml")),
            ("xl/drawings/drawing1.xml", drawing),
            ("xl/drawings/_rels/drawing1.xml.rels", &rels("chart", "../charts/chart1.xml")),
            ("xl/charts/chart1.xml", chart),
        ],
    );
    let file = temp_file(&workbook, ".xlsx");
    let path = file.path().to_str().unwrap();
    assert_eq!(tables_core::io::read_charts_from_xlsx(path).len(), 1, "the chart is read, at the default size");
    tables_core::io::load_workbook(path).expect("the workbook opens");

    eprintln!("size bombs: all read");
}

#[test]
fn a_count_or_range_that_multiplies_stays_in_the_sheet() {
    run_child("size_bombs_child", SIZE_BOMBS_CHILD, "size bombs", Some(4 << 20), std::time::Duration::from_secs(120));
}
