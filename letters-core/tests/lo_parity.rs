// lo_parity.rs — LibreOffice-authored parity corpus (ratcheted).
//
// "Being in the LibreOffice test suite" without vendoring MPL data files:
// each scenario below is authored as HTML, converted to .docx by headless
// LibreOffice Writer at test time, and then read by our engine. A scenario
// passes when we extract the expected text AND the expected styles from a
// document LibreOffice itself wrote. The pass count ratchets like the
// CommonMark corpus: dropping below baseline fails CI; raising it is
// visible parity progress.
//
// Skips without soffice unless REQUIRE_SOFFICE=1 (the CI oracle job sets it).

use std::process::Command;

use letters_core::docx;
use letters_core::model::{Alignment, Document};

struct Scenario {
    name: &'static str,
    html: &'static str,
    expected_text: &'static str,
    check: fn(&Document) -> Result<(), String>,
}

fn ok(_: &Document) -> Result<(), String> { Ok(()) }

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "plain-paragraphs",
            html: "<p>first paragraph</p><p>second paragraph</p>",
            expected_text: "first paragraph\nsecond paragraph",
            check: ok,
        },
        Scenario {
            name: "bold-run",
            html: "<p>before <b>bolded</b> after</p>",
            expected_text: "before bolded after",
            check: |d| {
                let s = d.style_at(7);
                if s.bold { Ok(()) } else { Err("bold not detected at offset 7".into()) }
            },
        },
        Scenario {
            name: "italic-run",
            html: "<p>an <i>italic</i> word</p>",
            expected_text: "an italic word",
            check: |d| if d.style_at(3).italic { Ok(()) } else { Err("italic lost".into()) },
        },
        Scenario {
            name: "underline-run",
            html: "<p>an <u>underlined</u> word</p>",
            expected_text: "an underlined word",
            check: |d| if d.style_at(3).underline { Ok(()) } else { Err("underline lost".into()) },
        },
        Scenario {
            name: "strikethrough-run",
            html: "<p>a <s>struck</s> word</p>",
            expected_text: "a struck word",
            check: |d| if d.style_at(2).strikethrough { Ok(()) } else { Err("strike lost".into()) },
        },
        Scenario {
            name: "nested-bold-italic",
            html: "<p><b><i>both</i></b> plain</p>",
            expected_text: "both plain",
            check: |d| {
                let s = d.style_at(0);
                if s.bold && s.italic { Ok(()) } else { Err(format!("want bold+italic, got {:?}", s)) }
            },
        },
        Scenario {
            name: "heading-1",
            html: "<h1>Big Title</h1><p>body</p>",
            expected_text: "Big Title\nbody",
            check: |d| {
                if d.paragraphs[0].style.heading == Some(1) { Ok(()) }
                else { Err(format!("heading: {:?}", d.paragraphs[0].style.heading)) }
            },
        },
        Scenario {
            name: "heading-3",
            html: "<h3>Sub</h3><p>body</p>",
            expected_text: "Sub\nbody",
            check: |d| {
                if d.paragraphs[0].style.heading == Some(3) { Ok(()) }
                else { Err(format!("heading: {:?}", d.paragraphs[0].style.heading)) }
            },
        },
        Scenario {
            name: "center-alignment",
            html: "<p style=\"text-align:center\">centered text</p>",
            expected_text: "centered text",
            check: |d| {
                if d.paragraphs[0].style.alignment == Alignment::Center { Ok(()) }
                else { Err(format!("alignment: {:?}", d.paragraphs[0].style.alignment)) }
            },
        },
        Scenario {
            name: "right-alignment",
            html: "<p style=\"text-align:right\">righted text</p>",
            expected_text: "righted text",
            check: |d| {
                if d.paragraphs[0].style.alignment == Alignment::Right { Ok(()) }
                else { Err(format!("alignment: {:?}", d.paragraphs[0].style.alignment)) }
            },
        },
        Scenario {
            name: "bullet-list-text",
            html: "<ul><li>alpha</li><li>beta</li></ul>",
            expected_text: "alpha\nbeta",
            check: ok, // list *kind* readback is a known red (rdocx getter)
        },
        Scenario {
            name: "numbered-list-text",
            html: "<ol><li>one</li><li>two</li></ol>",
            expected_text: "one\ntwo",
            check: ok,
        },
        Scenario {
            name: "hyperlink-text",
            html: "<p>go to <a href=\"https://gnome.org\">GNOME</a> now</p>",
            expected_text: "go to GNOME now",
            check: ok, // link readback is a known red (rdocx LinkInfo mapping)
        },
        Scenario {
            name: "unicode-content",
            html: "<p>caf\u{e9} — “smart” 中文</p>",
            expected_text: "café — “smart” 中文",
            check: ok,
        },
        Scenario {
            // LO's HTML import drops empty <p> elements — the ground truth
            // here is what LibreOffice authored, and it authored two
            // paragraphs. (Our own writer preserves empty paragraphs; that
            // is covered by tests/docx.rs.)
            name: "adjacent-paragraphs",
            html: "<p>above</p><p></p><p>below</p>",
            expected_text: "above\nbelow",
            check: ok,
        },
        Scenario {
            name: "simple-table-text",
            html: "<table><tr><td>a1</td><td>b1</td></tr><tr><td>a2</td><td>b2</td></tr></table>",
            expected_text: "a1\nb1\na2\nb2",
            check: ok, // tables not modeled yet; text must at least survive
        },
    ]
}

fn soffice_available() -> bool {
    Command::new("soffice").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn baseline() -> usize {
    include_str!("corpus/lo-parity-baseline.txt").trim().parse().expect("baseline int")
}

#[test]
fn libreoffice_parity_ratchet() {
    if !soffice_available() {
        if std::env::var("REQUIRE_SOFFICE").is_ok() {
            panic!("REQUIRE_SOFFICE set but no soffice binary found");
        }
        eprintln!("skipping: soffice not installed");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("lo-profile");
    let mut passed = 0usize;
    let mut results = Vec::new();

    for sc in scenarios() {
        let html_path = dir.path().join(format!("{}.html", sc.name));
        std::fs::write(&html_path, format!("<html><body>{}</body></html>", sc.html)).unwrap();

        let out = Command::new("soffice")
            .arg("--headless")
            .arg(format!("-env:UserInstallation=file://{}", profile.display()))
            .args(["--convert-to", "docx:MS Word 2007 XML", "--outdir"])
            .arg(dir.path())
            .arg(&html_path)
            .output()
            .expect("run soffice");
        let docx_path = html_path.with_extension("docx");
        if !out.status.success() || !docx_path.exists() {
            results.push((sc.name, Err("LibreOffice conversion failed".to_string())));
            continue;
        }

        let verdict = match docx::read(docx_path.to_str().unwrap()) {
            Err(e) => Err(format!("our reader failed: {e}")),
            Ok(doc) => {
                let got = doc.to_plain_text();
                // Trim outer empties: OOXML mandates a paragraph after each
                // table, and our table flattening appends at the end (see
                // docx.rs), so leading/trailing blanks are positional noise.
                let got_norm = got.trim().to_string();
                if got_norm != sc.expected_text {
                    Err(format!("text mismatch:\n  want {:?}\n  got  {:?}", sc.expected_text, got_norm))
                } else {
                    (sc.check)(&doc)
                }
            }
        };
        if verdict.is_ok() { passed += 1; }
        results.push((sc.name, verdict));
    }

    let total = results.len();
    println!("\nLibreOffice-authored parity: {passed}/{total}");
    for (name, r) in &results {
        match r {
            Ok(()) => println!("  PASS {name}"),
            Err(e) => println!("  FAIL {name}: {e}"),
        }
    }

    let base = baseline();
    assert!(
        passed >= base,
        "REGRESSION: LO parity dropped to {passed}/{total}, baseline {base}"
    );
    if passed > base {
        println!("IMPROVEMENT: {passed} > baseline {base} — bump tests/corpus/lo-parity-baseline.txt");
    }
}
