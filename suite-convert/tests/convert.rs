//! End to end: the built binary converts real files and exits with the
//! published codes.

use std::path::Path;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_suite-convert"))
        .args(args)
        .output()
        .unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}

fn write(dir: &Path, name: &str, text: &str) -> String {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn markdown_to_docx_to_markdown_keeps_the_text() {
    let dir = tempfile::tempdir().unwrap();
    let md = write(dir.path(), "a.md", "# Title\n\nSome **bold** words.\n");
    let docx = dir.path().join("a.docx").to_string_lossy().into_owned();
    let back = dir.path().join("b.md").to_string_lossy().into_owned();
    assert_eq!(code(&run(&[&md, &docx])), 0);
    assert!(Path::new(&docx).is_file());
    assert_eq!(code(&run(&[&docx, &back])), 0);
    let text = std::fs::read_to_string(&back).unwrap();
    assert!(
        text.contains("Title") && text.contains("**bold**"),
        "{text}"
    );
}

#[test]
fn markdown_to_html_writes_markup() {
    let dir = tempfile::tempdir().unwrap();
    let md = write(dir.path(), "a.md", "# Hi\n");
    let html = dir.path().join("a.html").to_string_lossy().into_owned();
    assert_eq!(code(&run(&[&md, &html])), 0);
    assert!(std::fs::read_to_string(&html).unwrap().contains("<h1"));
}

#[test]
fn a_missing_input_is_exit_one() {
    assert_eq!(code(&run(&["/no/such/file.md", "/tmp/x.docx"])), 1);
}

#[test]
fn bad_arguments_are_exit_one_and_an_unreadable_docx_is_exit_two() {
    assert_eq!(code(&run(&["--nope"])), 1);
    let dir = tempfile::tempdir().unwrap();
    let bad = write(dir.path(), "bad.docx", "this is not a zip");
    let out = dir.path().join("o.md").to_string_lossy().into_owned();
    assert_eq!(code(&run(&[&bad, &out])), 2);
}

#[test]
fn a_failure_writing_the_output_is_exit_three() {
    // Supported conversion, but the output cannot be written: an I/O failure.
    let dir = tempfile::tempdir().unwrap();
    let md = write(dir.path(), "a.md", "text\n");
    assert_eq!(code(&run(&[&md, "/no/such/dir/out.docx"])), 3);
}

#[test]
fn a_presentation_converts_to_an_approximate_pdf_and_a_png() {
    let dir = tempfile::tempdir().unwrap();
    let pptx = dir.path().join("a.pptx");
    let mut deck = decks_core::Deck::new();
    deck.slides[0].objects.push(decks_core::SlideObject::Rect {
        x: 40.0,
        y: 40.0,
        w: 200.0,
        h: 100.0,
        rotation: 0.0,
    });
    decks_core::write_deck(&pptx.to_string_lossy(), &deck).unwrap();
    let pptx = pptx.to_string_lossy().into_owned();

    let pdf = dir.path().join("a.pdf").to_string_lossy().into_owned();
    let out = run(&[&pptx, &pdf, "--report-loss"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
    // The report says the PDF is a re-drawing, not Decks' own render.
    assert!(String::from_utf8_lossy(&out.stderr).contains("typst-approximate"));

    let png = dir.path().join("a.png").to_string_lossy().into_owned();
    let out = run(&[&pptx, &png]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn spreadsheet_to_ods_is_refused_as_unsupported_because_tables_cannot_write_it() {
    let dir = tempfile::tempdir().unwrap();
    let csv = write(dir.path(), "a.csv", "a,b\n1,2\n");
    let ods = dir.path().join("a.ods").to_string_lossy().into_owned();
    assert_eq!(code(&run(&[&csv, &ods])), 1);
}

#[test]
fn csv_to_xlsx_and_back_keeps_the_values_and_formulas_evaluate() {
    let dir = tempfile::tempdir().unwrap();
    let csv = write(
        dir.path(),
        "a.csv",
        "name,qty\n\"Smith, John\",2\nsum,=B2+3\n",
    );
    let xlsx = dir.path().join("a.xlsx").to_string_lossy().into_owned();
    let back = dir.path().join("b.csv").to_string_lossy().into_owned();
    assert_eq!(code(&run(&[&csv, &xlsx])), 0);
    assert_eq!(code(&run(&[&xlsx, &back])), 0);
    let text = std::fs::read_to_string(&back).unwrap();
    assert_eq!(text, "name,qty\r\n\"Smith, John\",2\r\nsum,5\r\n");
}

#[test]
fn report_loss_prints_json_and_the_budget_gates_the_exit() {
    let dir = tempfile::tempdir().unwrap();
    let md = write(dir.path(), "a.md", "plain\n");
    let out = dir.path().join("a.odt").to_string_lossy().into_owned();
    let o = run(&[&md, &out, "--report-loss", "--fail-on-loss", "none"]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_slice(&o.stderr).unwrap();
    assert_eq!(v["from"], "md");
    assert_eq!(v["to"], "odt");
    assert!(v["losses"].is_array());
}

#[test]
fn a_spreadsheet_converts_to_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let csv = write(dir.path(), "a.csv", "a,b\n1,2\n");
    let pdf = dir.path().join("a.pdf").to_string_lossy().into_owned();
    let o = run(&[&csv, &pdf]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
}

#[test]
fn a_presentation_round_trips_between_pptx_and_odp() {
    let dir = tempfile::tempdir().unwrap();
    let pptx = dir.path().join("a.pptx");
    let deck = decks_core::Deck::new();
    decks_core::write_deck(&pptx.to_string_lossy(), &deck).unwrap();
    let odp = dir.path().join("a.odp").to_string_lossy().into_owned();
    let back = dir.path().join("b.pptx").to_string_lossy().into_owned();
    let o = run(&[&pptx.to_string_lossy(), &odp]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(code(&run(&[&odp, &back])), 0);
    assert!(std::path::Path::new(&back).is_file());
}

#[test]
fn a_spreadsheet_converts_to_escaped_html() {
    let dir = tempfile::tempdir().unwrap();
    let csv = write(dir.path(), "a.csv", "x,y\n\"<b>\",2\n");
    let html = dir.path().join("a.html").to_string_lossy().into_owned();
    let o = run(&[&csv, &html]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    let text = std::fs::read_to_string(&html).unwrap();
    assert!(
        text.contains("<td>&lt;b&gt;</td>") && text.contains("<td>2</td>"),
        "{text}"
    );
}

#[test]
fn a_text_document_converts_to_an_approximate_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let md = write(dir.path(), "a.md", "# Title\n\nSome text with $5 and #1.\n");
    let pdf = dir.path().join("a.pdf").to_string_lossy().into_owned();
    let o = run(&[&md, &pdf, "--report-loss"]);
    assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
    // The report says it is approximate, so a zero loss budget refuses it.
    assert!(String::from_utf8_lossy(&o.stderr).contains("typst-approximate"));
    assert_eq!(code(&run(&[&md, &pdf, "--fail-on-loss", "none"])), 4);
}

#[test]
fn a_document_and_a_spreadsheet_convert_to_a_png_of_the_asked_width() {
    let dir = tempfile::tempdir().unwrap();
    let md = write(dir.path(), "a.md", "# Title\n\nBody text.\n");
    let csv = write(dir.path(), "a.csv", "a,b\n1,2\n");
    for (input, name) in [(md, "a.png"), (csv, "b.png")] {
        let out = dir.path().join(name).to_string_lossy().into_owned();
        let o = run(&[&input, &out, "--size", "200"]);
        assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        // The IHDR width is bytes 16..20, big endian.
        assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 200);
    }
}

#[test]
fn a_page_past_the_end_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let md = write(dir.path(), "a.md", "one page\n");
    let out = dir.path().join("a.png").to_string_lossy().into_owned();
    assert_eq!(code(&run(&[&md, &out, "--page", "9"])), 1);
}
