fn main() {
    let p = std::env::args().nth(1).unwrap();
    let d0 = letters_core::docx::read(&p).unwrap();
    letters_core::save::write(&d0, std::path::Path::new("/tmp/rtprobe2.docx")).unwrap();
    let d1 = letters_core::docx::read("/tmp/rtprobe2.docx").unwrap();
    for (name, d) in [("BEFORE", &d0), ("AFTER", &d1)] {
        println!("== {name}");
        for (i, para) in d.paragraphs.iter().enumerate() {
            let t: Vec<String> = para.runs.iter().map(|r| format!("{}{:?}", if r.style.image.is_some() { "[img]" } else { "" }, r.text.chars().take(24).collect::<String>())).collect();
            println!("{i:2} tc={:?} {}", para.style.table_cell.as_ref().map(|c| (c.row, c.col)), t.join(" "));
        }
    }
}
