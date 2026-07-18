// cm_probe — print failing CommonMark round-trip examples for a section.
// Dev tool for the ratchet: `cargo run -p letters-core --example cm_probe -- "Emphasis"`.

use letters_core::markdown;

#[derive(serde::Deserialize)]
struct Example {
    markdown: String,
    section: String,
    example: u32,
}

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default();
    let raw = include_str!("../tests/corpus/commonmark-spec.json");
    let examples: Vec<Example> = serde_json::from_str(raw).expect("corpus");
    let mut shown = 0;
    for ex in &examples {
        if !ex.section.contains(&filter) {
            continue;
        }
        let doc1 = markdown::parse(&ex.markdown);
        let md2 = markdown::serialize(&doc1);
        let doc2 = markdown::parse(&md2);
        if doc1 != doc2 {
            shown += 1;
            println!("── example {} ({}) ──", ex.example, ex.section);
            println!("input:  {:?}", ex.markdown);
            println!("re-ser: {:?}", md2);
            if shown >= usize::MAX {
                break;
            }
        }
    }
    println!("{shown} failing in sections matching {filter:?}");
}
