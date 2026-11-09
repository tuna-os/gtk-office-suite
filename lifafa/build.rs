//! Compile the bundled icon set (resources/hylki.gresource.xml) into a
//! `.gresource` blob that is embedded in the binary and registered at startup
//! (see `main.rs`). The icons are named plainly, so the user's icon theme draws
//! those it has; the bundle supplies Hylki's own and any a theme lacks, so none
//! goes missing on any distribution (#260).

fn main() {
    println!("cargo:rerun-if-changed=resources/hylki.gresource.xml");
    println!("cargo:rerun-if-changed=resources/icons");
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/hylki.gresource.xml",
        "hylki.gresource",
    );
    // The bundled sender logos (data/logos/, listed by tools/fetch-logos.py).
    println!("cargo:rerun-if-changed=resources/logos.gresource.xml");
    println!("cargo:rerun-if-changed=data/logos");
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/logos.gresource.xml",
        "logos.gresource",
    );
    // The built-in new-mail sounds (data/sounds/).
    println!("cargo:rerun-if-changed=resources/sounds.gresource.xml");
    println!("cargo:rerun-if-changed=data/sounds");
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/sounds.gresource.xml",
        "sounds.gresource",
    );
}
