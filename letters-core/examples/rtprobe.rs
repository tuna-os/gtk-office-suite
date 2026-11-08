use serde_json::Value;
fn diff(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                diff(&format!("{path}.{k}"), x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null), out);
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() { out.push(format!("{path}: len {} -> {}", x.len(), y.len())); }
            for i in 0..x.len().min(y.len()) { diff(&format!("{path}[{i}]"), &x[i], &y[i], out); }
        }
        _ => if a != b { out.push(format!("{path}: {} -> {}", a.to_string().chars().take(90).collect::<String>(), b.to_string().chars().take(90).collect::<String>())); }
    }
}
fn main() {
    for p in std::env::args().skip(1) {
        let odt = p.ends_with(".odt");
        let read = |f: &str| if odt { letters_core::odt::read(f) } else { letters_core::docx::read(f) };
        let d0 = read(&p).unwrap();
        let tmp = format!("/tmp/rtprobe.{}", if odt { "odt" } else { "docx" });
        letters_core::save::write(&d0, std::path::Path::new(&tmp)).unwrap();
        let d1 = read(&tmp).unwrap();
        let (a, b) = (serde_json::to_value(&d0).unwrap(), serde_json::to_value(&d1).unwrap());
        let mut out = vec![];
        diff("", &a, &b, &mut out);
        println!("== {p}: {} paragraphs, {} differences", d0.paragraphs.len(), out.len());
        for l in out.iter().take(40) { println!("   {l}"); }
    }
}
