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
fn an_engine_that_is_not_there_yet_is_exit_three() {
    let dir = tempfile::tempdir().unwrap();
    let csv = write(dir.path(), "a.csv", "a,b\n1,2\n");
    let ods = dir.path().join("a.ods").to_string_lossy().into_owned();
    assert_eq!(code(&run(&[&csv, &ods])), 3);
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
