//! Compile suite-common/icons/ into a `.gresource` blob embedded in the
//! library and registered by `icons::install`. The resource list is written
//! from the directory, so adding an icon is adding its SVG.

use std::fmt::Write as _;

fn main() {
    println!("cargo:rerun-if-changed=icons");
    let mut names: Vec<String> = std::fs::read_dir("icons")
        .expect("suite-common/icons")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".svg"))
        .collect();
    names.sort();
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gresources>\n  <gresource prefix=\"/org/tunaos/suite/icons/scalable/actions\">\n",
    );
    for name in &names {
        writeln!(xml, "    <file>{name}</file>").unwrap();
    }
    xml.push_str("  </gresource>\n</gresources>\n");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("suite-icons.gresource.xml");
    std::fs::write(&out, xml).unwrap();
    glib_build_tools::compile_resources(&["icons"], out.to_str().unwrap(), "suite-icons.gresource");
}
