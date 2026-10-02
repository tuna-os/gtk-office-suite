// CommonMark corpus harness — round-trip idempotence ratchet.
//
// For every example in the vendored CommonMark spec (652 inputs), we check
// parse(md) == parse(serialize(parse(md))). This is not HTML conformance
// (we are a document model, not an HTML renderer); it measures that our
// Markdown reader/writer pair is *stable* on the full grammar the spec
// exercises. The pass count may never drop below the recorded baseline —
// raising it is progress you can see in CI output.

use letters_core::markdown;

#[derive(serde::Deserialize)]
struct Example {
    markdown: String,
    section: String,
    example: u32,
}

fn corpus() -> Vec<Example> {
    let raw = include_str!("corpus/commonmark-spec.json");
    serde_json::from_str(raw).expect("corpus JSON")
}

fn baseline() -> usize {
    include_str!("corpus/roundtrip-baseline.txt").trim().parse().expect("baseline int")
}

#[test]
fn commonmark_roundtrip_ratchet() {
    let examples = corpus();
    let total = examples.len();
    let mut passed = 0usize;
    let mut per_section: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    let mut failures: Vec<u32> = Vec::new();

    for ex in &examples {
        let doc1 = markdown::parse(&ex.markdown);
        let md2 = markdown::serialize(&doc1);
        let doc2 = markdown::parse(&md2);
        let ok = doc1 == doc2;
        let entry = per_section.entry(ex.section.clone()).or_default();
        entry.1 += 1;
        if ok {
            entry.0 += 1;
            passed += 1;
        } else {
            failures.push(ex.example);
        }
    }

    println!("\nCommonMark round-trip idempotence: {passed}/{total}");
    for (section, (ok, n)) in &per_section {
        println!("  {section:<28} {ok:>3}/{n:<3}");
    }

    let base = baseline();
    assert!(
        passed >= base,
        "REGRESSION: round-trip passes dropped to {passed}, baseline is {base}. \
         First failing examples: {:?}",
        &failures[..failures.len().min(10)]
    );
    if passed > base {
        println!(
            "IMPROVEMENT: {passed} > baseline {base} — bump tests/corpus/roundtrip-baseline.txt"
        );
    }
}
