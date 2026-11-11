//! The point of this crate (HEADLESS-CONVERSION-SPEC, phase 1): it links the
//! core crates and cannot link GTK, so the architecture rule is a build error
//! rather than a convention. This asserts it on the resolved dependency graph.

use std::process::Command;

const GTK_STACK: &[&str] = &[
    "gtk4",
    "gtk4-sys",
    "libadwaita",
    "libadwaita-sys",
    "gdk4",
    "gdk4-sys",
    "gdk-pixbuf",
    "gsk4",
    "pango",
    "pango-sys",
    "pangocairo",
    "cairo-rs",
    "graphene-rs",
    "glib",
    "glib-sys",
    "gio",
    "gio-sys",
];

#[test]
fn the_dependency_graph_holds_no_gtk() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = match Command::new(&cargo)
        .args([
            "tree",
            "-p",
            "suite-convert",
            "--prefix",
            "none",
            "--edges",
            "normal,build",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o,
        // No cargo or no network to resolve: say so rather than pass silently.
        Ok(o) => panic!(
            "`cargo tree` failed: {}",
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => panic!("could not run `{cargo} tree`: {e}"),
    };
    let tree = String::from_utf8_lossy(&output.stdout);
    assert!(
        tree.lines()
            .next()
            .is_some_and(|l| l.starts_with("suite-convert ")),
        "unexpected tree:\n{tree}"
    );
    for line in tree.lines() {
        let name = line.split_whitespace().next().unwrap_or("");
        assert!(
            !GTK_STACK.contains(&name),
            "`{name}` is in suite-convert's dependency graph:\n{tree}"
        );
    }
}
