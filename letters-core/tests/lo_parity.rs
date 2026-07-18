// lo_parity.rs — LibreOffice-authored parity corpus (ratcheted).
//
// "Being in the LibreOffice test suite" without vendoring MPL data files:
// each scenario is authored as HTML, converted to .docx by headless
// LibreOffice Writer at test time (one batched soffice launch), then read
// by our engine. A scenario passes when we extract the expected text AND
// the expected styles from a document LibreOffice itself wrote. The pass
// count ratchets: dropping below baseline fails CI; raising it is visible
// parity progress.
//
// Scenarios are generated as feature batteries (inline styles × positions,
// headings, alignments, lists, tables, unicode, structure) so the corpus
// grows multiplicatively — 100+ cases from a few loops.
//
// Skips without soffice unless REQUIRE_SOFFICE=1 (the CI oracle job sets it).

use std::process::Command;

use letters_core::docx;
use letters_core::model::{Alignment, Document};

type Check = Box<dyn Fn(&Document) -> Result<(), String>>;

struct Scenario {
    name: String,
    html: String,
    expected_text: String,
    check: Check,
}

fn ok() -> Check { Box::new(|_| Ok(())) }

fn sc(name: impl Into<String>, html: impl Into<String>, expected: impl Into<String>, check: Check) -> Scenario {
    Scenario { name: name.into(), html: html.into(), expected_text: expected.into(), check }
}

fn text_only(name: impl Into<String>, html: impl Into<String>, expected: impl Into<String>) -> Scenario {
    sc(name, html, expected, ok())
}

/// Check that a given style attribute holds exactly on [start, end) and
/// nowhere adjacent.
fn style_range(attr: &'static str, start: usize, end: usize) -> Check {
    Box::new(move |d| {
        let has = |o: usize| -> bool {
            let s = d.style_at(o);
            match attr {
                "bold" => s.bold,
                "italic" => s.italic,
                "underline" => s.underline,
                "strikethrough" => s.strikethrough,
                "code" => s.code,
                _ => unreachable!(),
            }
        };
        if start > 0 && has(start - 1) {
            return Err(format!("{attr} leaks before {start}"));
        }
        for o in start..end {
            if !has(o) { return Err(format!("{attr} missing at {o}")); }
        }
        // Only check the trailing boundary when a character exists there:
        // at paragraph end, style_at intentionally inherits the last run's
        // style (so typing continues the current style).
        if end < d.paragraphs[0].char_len() && has(end) {
            return Err(format!("{attr} leaks past {end}"));
        }
        Ok(())
    })
}

fn heading_check(levels: Vec<Option<u8>>) -> Check {
    Box::new(move |d| {
        for (i, want) in levels.iter().enumerate() {
            let got = d.paragraphs.get(i).and_then(|p| p.style.heading);
            if got != *want { return Err(format!("para {i}: heading {got:?} != {want:?}")); }
        }
        Ok(())
    })
}

fn alignment_check(idx: usize, want: Alignment) -> Check {
    Box::new(move |d| {
        let got = d.paragraphs.get(idx).map(|p| p.style.alignment);
        if got == Some(want) { Ok(()) } else { Err(format!("alignment {got:?} != {want:?}")) }
    })
}

fn scenarios() -> Vec<Scenario> {
    let mut v: Vec<Scenario> = Vec::new();

    // ── Battery 1: inline styles × positions ────────────────────────────
    // 5 styles × 4 positions = 20 scenarios with exact-boundary checks.
    let styles: [(&str, &str, &str); 5] = [
        ("bold", "b", "bold"),
        ("italic", "i", "italic"),
        ("underline", "u", "underline"),
        ("strike", "s", "strikethrough"),
        ("code-span", "code", "code"),
    ];
    for (label, tag, attr) in styles {
        // whole paragraph styled: "styled words" (12 chars)
        v.push(sc(
            format!("inline-{label}-whole"),
            format!("<p><{tag}>styled words</{tag}></p>"),
            "styled words",
            style_range(attr, 0, 12),
        ));
        // at start: "hot cold" styled [0,3)
        v.push(sc(
            format!("inline-{label}-start"),
            format!("<p><{tag}>hot</{tag}> cold</p>"),
            "hot cold",
            style_range(attr, 0, 3),
        ));
        // at end: "cold hot" styled [5,8)
        v.push(sc(
            format!("inline-{label}-end"),
            format!("<p>cold <{tag}>hot</{tag}></p>"),
            "cold hot",
            style_range(attr, 5, 8),
        ));
        // mid-word: "unbelievable" styled [2,8)
        v.push(sc(
            format!("inline-{label}-midword"),
            format!("<p>un<{tag}>believ</{tag}>able</p>"),
            "unbelievable",
            style_range(attr, 2, 8),
        ));
    }

    // ── Battery 2: nested style pairs ───────────────────────────────────
    // 6 pairs, both attributes must hold on the styled span.
    let pairs: [(&str, &str); 6] =
        [("b", "i"), ("b", "u"), ("b", "s"), ("i", "u"), ("i", "s"), ("u", "s")];
    for (outer, inner) in pairs {
        let name = format!("nested-{outer}-{inner}");
        let html = format!("<p>pre <{outer}><{inner}>core</{inner}></{outer}> post</p>");
        v.push(sc(name, html, "pre core post", Box::new(move |d| {
            let s = d.style_at(5);
            let want = |t: &str, on: bool| -> Result<(), String> {
                let got = match t {
                    "b" => s.bold, "i" => s.italic, "u" => s.underline, "s" => s.strikethrough,
                    _ => unreachable!(),
                };
                if got == on { Ok(()) } else { Err(format!("{t} = {got}, want {on}")) }
            };
            want(outer, true)?;
            want(inner, true)?;
            if d.style_at(0).bold || d.style_at(0).italic || d.style_at(0).underline || d.style_at(0).strikethrough {
                return Err("styles leak into prefix".into());
            }
            Ok(())
        })));
    }

    // ── Battery 3: headings 1–6, plain and with styled content ──────────
    for level in 1u8..=6 {
        v.push(sc(
            format!("heading-{level}"),
            format!("<h{level}>Heading Text</h{level}><p>body</p>"),
            "Heading Text\nbody",
            heading_check(vec![Some(level), None]),
        ));
        v.push(sc(
            format!("heading-{level}-with-italic"),
            format!("<h{level}>plain <i>slanted</i></h{level}><p>body</p>"),
            "plain slanted\nbody",
            heading_check(vec![Some(level), None]),
        ));
    }

    // ── Battery 4: alignments ───────────────────────────────────────────
    for (name, css, want) in [
        ("center", "center", Alignment::Center),
        ("right", "right", Alignment::Right),
        ("justify", "justify", Alignment::Justify),
    ] {
        v.push(sc(
            format!("align-{name}"),
            format!("<p style=\"text-align:{css}\">aligned body text</p>"),
            "aligned body text",
            alignment_check(0, want),
        ));
        v.push(sc(
            format!("align-{name}-second-para"),
            format!("<p>first</p><p style=\"text-align:{css}\">second</p>"),
            "first\nsecond",
            alignment_check(1, want),
        ));
    }

    // ── Battery 5: paragraph structure ──────────────────────────────────
    for n in [2usize, 3, 5, 8] {
        let html: String = (1..=n).map(|i| format!("<p>paragraph {i}</p>")).collect();
        let expected = (1..=n).map(|i| format!("paragraph {i}")).collect::<Vec<_>>().join("\n");
        v.push(text_only(format!("structure-{n}-paragraphs"), html, expected));
    }
    v.push(text_only("structure-long-paragraph",
        format!("<p>{}</p>", "long sentence ".repeat(40).trim()),
        "long sentence ".repeat(40).trim().to_string()));
    v.push(text_only("structure-hard-break", "<p>line one<br>line two</p>", "line one\nline two"));
    v.push(text_only("structure-two-breaks", "<p>a<br>b<br>c</p>", "a\nb\nc"));
    v.push(text_only("structure-hr", "<p>before</p><hr><p>after</p>", "before\nafter"));

    // ── Battery 6: lists ────────────────────────────────────────────────
    for (kind, tag) in [("bullet", "ul"), ("numbered", "ol")] {
        for n in [1usize, 2, 4] {
            let items: String = (1..=n).map(|i| format!("<li>item {i}</li>")).collect();
            let expected = (1..=n).map(|i| format!("item {i}")).collect::<Vec<_>>().join("\n");
            v.push(text_only(format!("list-{kind}-{n}-items"), format!("<{tag}>{items}</{tag}>"), expected));
        }
        v.push(text_only(
            format!("list-{kind}-styled-item"),
            format!("<{tag}><li>plain <b>bolded</b></li></{tag}>"),
            "plain bolded",
        ));
    }
    v.push(text_only("list-nested",
        "<ul><li>outer<ul><li>inner</li></ul></li><li>outer two</li></ul>",
        "outer\ninner\nouter two"));
    v.push(text_only("list-then-paragraph",
        "<ul><li>item</li></ul><p>afterwards</p>", "item\nafterwards"));

    // ── Battery 7: links ────────────────────────────────────────────────
    v.push(text_only("link-basic",
        "<p>go to <a href=\"https://gnome.org\">GNOME</a> now</p>", "go to GNOME now"));
    v.push(text_only("link-whole-paragraph",
        "<p><a href=\"https://example.com\">entire link line</a></p>", "entire link line"));
    v.push(text_only("link-two-in-one",
        "<p><a href=\"https://a.example\">first</a> and <a href=\"https://b.example\">second</a></p>",
        "first and second"));

    // ── Battery 8: unicode & special content ────────────────────────────
    for (name, content) in [
        ("accents", "café naïve résumé"),
        ("cjk", "中文测试 日本語 한국어"),
        ("rtl-arabic", "مرحبا بالعالم"),
        ("rtl-hebrew", "שלום עולם"),
        ("smart-punct", "“curly” ‘quotes’ — em–en… dashes"),
        ("math-symbols", "∑ ∫ √ ≈ ≠ ∞ π"),
        ("emoji", "rocket 🚀 sparkles ✨"),
        ("currency", "€100 £75 ¥500 ₹250"),
        ("md-metachars", "*not markdown* _nor this_ [nor](this) #tag"),
        ("xml-metachars", "a < b && c > d \"quoted\""),
    ] {
        v.push(text_only(format!("unicode-{name}"), format!("<p>{}</p>",
            content.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")), content));
    }

    // ── Battery 9: tables ───────────────────────────────────────────────
    v.push(text_only("table-1x1", "<table><tr><td>lone</td></tr></table>", "lone"));
    v.push(sc("table-2x2",
        "<table><tr><td>a1</td><td>b1</td></tr><tr><td>a2</td><td>b2</td></tr></table>",
        "a1\nb1\na2\nb2",
        Box::new(|d| {
            // Structure, not just text: every cell at its coordinates.
            let cells: Vec<(u32, u32, String)> = d.paragraphs.iter()
                .filter_map(|p| p.style.table_cell.map(|tc| (tc.row, tc.col, p.text())))
                .collect();
            for want in [(0u32, 0u32, "a1"), (0, 1, "b1"), (1, 0, "a2"), (1, 1, "b2")] {
                if !cells.contains(&(want.0, want.1, want.2.to_string())) {
                    return Err(format!("missing cell {want:?}; got {cells:?}"));
                }
            }
            Ok(())
        })));
    v.push(text_only("table-3x2",
        "<table><tr><td>r1c1</td><td>r1c2</td></tr><tr><td>r2c1</td><td>r2c2</td></tr><tr><td>r3c1</td><td>r3c2</td></tr></table>",
        "r1c1\nr1c2\nr2c1\nr2c2\nr3c1\nr3c2"));
    v.push(sc("table-styled-cell",
        "<table><tr><td>plain <b>bolded</b></td></tr></table>", "plain bolded",
        Box::new(|d| {
            let para = d.paragraphs.iter().find(|p| p.text().contains("bolded"))
                .ok_or("cell text missing")?;
            let run = para.runs.iter().find(|r| r.text.contains("bolded"))
                .ok_or("no run containing 'bolded'")?;
            if run.style.bold { Ok(()) } else { Err("cell bold lost".into()) }
        })));
    v.push(text_only("table-with-header-row",
        "<table><tr><th>Name</th><th>Qty</th></tr><tr><td>apples</td><td>5</td></tr></table>",
        "Name\nQty\napples\n5"));
    v.push(text_only("table-after-paragraph",
        "<p>intro</p><table><tr><td>cell</td></tr></table>", "intro\ncell"));

    // ── Battery 10: block elements ──────────────────────────────────────
    v.push(sc("pre-block", "<pre>let x = 1;</pre>", "let x = 1;",
        Box::new(|d| {
            if d.paragraphs[0].style.code_block.is_some() { Ok(()) }
            else { Err("pre not mapped to code_block".into()) }
        })));
    v.push(sc("pre-block-multiline", "<pre>line a\nline b</pre>", "line a\nline b",
        Box::new(|d| {
            if d.paragraphs.iter().take(2).all(|p| p.style.code_block.is_some()) { Ok(()) }
            else { Err("multiline pre not fully code_block".into()) }
        })));
    v.push(text_only("blockquote", "<blockquote><p>quoted wisdom</p></blockquote>", "quoted wisdom"));
    v.push(text_only("sup-sub", "<p>x<sup>2</sup> and H<sub>2</sub>O</p>", "x2 and H2O"));
    v.push(text_only("adjacent-paragraphs", "<p>above</p><p></p><p>below</p>", "above\nbelow"));
    v.push(text_only("definition-list", "<dl><dt>term</dt><dd>definition</dd></dl>", "term\ndefinition"));

    // ── Battery 11: mixed documents ─────────────────────────────────────
    v.push(sc("mixed-article",
        "<h1>Title</h1><p>intro with <b>bold</b></p><h2>Section</h2><ul><li>point one</li><li>point two</li></ul><p>closing</p>",
        "Title\nintro with bold\nSection\npoint one\npoint two\nclosing",
        heading_check(vec![Some(1), None, Some(2)])));
    v.push(sc("mixed-report",
        "<h2>Report</h2><p style=\"text-align:center\">centered abstract</p><p>body <i>emphasis</i> text</p>",
        "Report\ncentered abstract\nbody emphasis text",
        alignment_check(1, Alignment::Center)));
    v.push(text_only("mixed-notes",
        "<h3>Notes</h3><ol><li>first</li><li>second</li></ol><pre>code sample</pre>",
        "Notes\nfirst\nsecond\ncode sample"));

    // ── Battery 13: run-level typography ────────────────────────────────
    v.push(sc("font-size", "<p><span style=\"font-size:24pt\">huge</span> normal</p>", "huge normal",
        Box::new(|d| {
            let r = d.paragraphs[0].runs.iter().find(|r| r.text.contains("huge")).ok_or("run")?;
            match r.style.font_size_hp {
                Some(hp) if hp >= 44 => Ok(()),
                other => Err(format!("font size: {other:?}")),
            }
        })));
    v.push(sc("text-color", "<p><font color=\"#ff0000\">red</font> plain</p>", "red plain",
        Box::new(|d| {
            let r = d.paragraphs[0].runs.iter().find(|r| r.text.contains("red")).ok_or("run")?;
            match r.style.color.as_deref() {
                Some(c) if c.eq_ignore_ascii_case("ff0000") => Ok(()),
                other => Err(format!("color: {other:?}")),
            }
        })));
    v.push(sc("superscript", "<p>x<sup>2</sup></p>", "x2",
        Box::new(|d| {
            let r = d.paragraphs[0].runs.iter().find(|r| r.text == "2").ok_or("run")?;
            if r.style.vert_align == Some(letters_core::model::VertAlign::Superscript) { Ok(()) }
            else { Err(format!("vert: {:?}", r.style.vert_align)) }
        })));
    v.push(sc("subscript", "<p>H<sub>2</sub>O</p>", "H2O",
        Box::new(|d| {
            let r = d.paragraphs[0].runs.iter().find(|r| r.text == "2").ok_or("run")?;
            if r.style.vert_align == Some(letters_core::model::VertAlign::Subscript) { Ok(()) }
            else { Err(format!("vert: {:?}", r.style.vert_align)) }
        })));
    v.push(sc("blockquote-style", "<blockquote><p>quoted wisdom here</p></blockquote>", "quoted wisdom here",
        Box::new(|d| {
            if d.paragraphs[0].style.block_quote { Ok(()) }
            else { Err(format!("not marked quote; style_id path: {:?}", d.paragraphs[0].style)) }
        })));

    // ── Battery 12: edge cases & combinations ───────────────────────────
    v.push(text_only("edge-nbsp", "<p>a&nbsp;b</p>", "a\u{a0}b"));
    v.push(text_only("edge-whitespace-collapse", "<p>a   b</p>", "a b"));
    v.push(text_only("edge-ampersand-entities", "<p>fish &amp; chips &copy; 2026</p>", "fish & chips © 2026"));
    v.push(text_only("edge-combining-diacritic", "<p>e\u{301}tude</p>", "e\u{301}tude"));
    v.push(text_only("table-1x4",
        "<table><tr><td>a</td><td>b</td><td>c</td><td>d</td></tr></table>", "a\nb\nc\nd"));
    v.push(text_only("table-4x1",
        "<table><tr><td>a</td></tr><tr><td>b</td></tr><tr><td>c</td></tr><tr><td>d</td></tr></table>",
        "a\nb\nc\nd"));
    v.push(text_only("table-two-tables",
        "<table><tr><td>first</td></tr></table><p>mid</p><table><tr><td>second</td></tr></table>",
        "mid\nfirst\nsecond")); // flattening appends tables after body text
    v.push(text_only("list-eight-items",
        &format!("<ul>{}</ul>", (1..=8).map(|i| format!("<li>i{i}</li>")).collect::<String>()),
        (1..=8).map(|i| format!("i{i}")).collect::<Vec<_>>().join("\n")));
    v.push(sc("heading-then-list",
        "<h2>Agenda</h2><ol><li>alpha</li><li>beta</li></ol>",
        "Agenda\nalpha\nbeta",
        heading_check(vec![Some(2), None, None])));
    v.push(text_only("link-with-bold-text",
        "<p><a href=\"https://x.example\">has <b>bold</b> inside</a></p>", "has bold inside"));
    v.push(text_only("pre-with-symbols", "<pre>if (a &lt; b) { return; }</pre>", "if (a < b) { return; }"));
    v.push(sc("styled-across-break",
        "<p><b>bold line<br>continues</b></p>", "bold line\ncontinues",
        Box::new(|d| {
            if d.style_at(0).bold && d.style_at(10).bold { Ok(()) }
            else { Err("bold lost across hard break".into()) }
        })));
    v.push(text_only("deep-nested-list",
        "<ul><li>l1<ul><li>l2<ul><li>l3</li></ul></li></ul></li></ul>", "l1\nl2\nl3"));
    v.push(sc("mixed-full-document",
        "<h1>Doc</h1><p style=\"text-align:center\">subtitle</p><h2>A</h2><p>text <b>b</b> <i>i</i> <u>u</u></p><ul><li>one</li></ul><pre>code</pre><table><tr><td>cell</td></tr></table>",
        "Doc\nsubtitle\nA\ntext b i u\none\ncode\ncell",
        heading_check(vec![Some(1), None, Some(2)])));

    v
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

    let scenarios = scenarios();
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("lo-profile");

    // Author every scenario as HTML, then convert the whole batch in a
    // single soffice launch (per-file launches would take minutes).
    let mut html_paths = Vec::new();
    for s in &scenarios {
        let p = dir.path().join(format!("{}.html", s.name));
        std::fs::write(&p, format!("<html><body>{}</body></html>", s.html)).unwrap();
        html_paths.push(p);
    }
    let out = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "docx:MS Word 2007 XML", "--outdir"])
        .arg(dir.path())
        .args(&html_paths)
        .output()
        .expect("run soffice");
    if !out.status.success() {
        panic!("batch conversion failed: {}", String::from_utf8_lossy(&out.stderr));
    }

    let mut passed = 0usize;
    let mut failures = Vec::new();
    let total = scenarios.len();

    for s in &scenarios {
        let docx_path = dir.path().join(format!("{}.docx", s.name));
        let verdict = if !docx_path.exists() {
            Err("LibreOffice produced no output".to_string())
        } else {
            match docx::read(docx_path.to_str().unwrap()) {
                Err(e) => Err(format!("our reader failed: {e}")),
                Ok(doc) => {
                    // Trim outer empties: OOXML mandates a paragraph after
                    // each table, and table flattening appends at the end.
                    let got = doc.to_plain_text().trim().to_string();
                    if got != s.expected_text {
                        Err(format!("text mismatch:\n    want {:?}\n    got  {:?}", s.expected_text, got))
                    } else {
                        (s.check)(&doc)
                    }
                }
            }
        };
        match verdict {
            Ok(()) => passed += 1,
            Err(e) => failures.push((s.name.clone(), e)),
        }
    }

    println!("\nLibreOffice-authored parity: {passed}/{total}");
    for (name, e) in &failures {
        println!("  FAIL {name}: {e}");
    }

    let base = baseline();
    assert!(passed >= base, "REGRESSION: LO parity dropped to {passed}/{total}, baseline {base}");
    if passed > base {
        println!("IMPROVEMENT: {passed} > baseline {base} — bump tests/corpus/lo-parity-baseline.txt");
    }
}
