// Debug harness: print failing corpus examples (not a gating test).
use letters_core::markdown;

#[derive(serde::Deserialize)]
struct Example { markdown: String, section: String, example: u32 }

#[test]
#[ignore]
fn dump_failures() {
    let raw = include_str!("corpus/commonmark-spec.json");
    let examples: Vec<Example> = serde_json::from_str(raw).unwrap();
    for ex in &examples {
        let doc1 = markdown::parse(&ex.markdown);
        let md2 = markdown::serialize(&doc1);
        let doc2 = markdown::parse(&md2);
        if doc1 != doc2 {
            println!("=== example {} [{}]", ex.example, ex.section);
            println!("--- input:\n{:?}", ex.markdown);
            println!("--- serialized:\n{:?}", md2);
            println!("--- doc1: {:#?}", doc1.paragraphs.iter().take(4).collect::<Vec<_>>());
            println!("--- doc2: {:#?}", doc2.paragraphs.iter().take(4).collect::<Vec<_>>());
        }
    }
}
