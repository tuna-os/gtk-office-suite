//! The `.thumbnailer` entry (INTEGRATION-PLAN B2) lists only what this binary
//! can draw: every MIME type in it must map to a format that converts to PNG,
//! and the command line it gives must parse and plan.

use suite_convert::{parse, plan, supported, Command, Format};

const ENTRY: &str = include_str!("../../flatpak/org.tunaos.suite.thumbnailer");

fn key(name: &str) -> &'static str {
    ENTRY
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{name}=")))
        .unwrap_or_else(|| panic!("no `{name}=` line"))
}

fn format_for(mime: &str) -> Option<Format> {
    Some(match mime {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Format::Docx,
        "application/vnd.oasis.opendocument.text" => Format::Odt,
        "text/markdown" => Format::Md,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Format::Xlsx,
        "application/vnd.oasis.opendocument.spreadsheet" => Format::Ods,
        "text/csv" => Format::Csv,
        "application/vnd.oasis.opendocument.presentation" => Format::Odp,
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => Format::Pptx,
        _ => return None,
    })
}

#[test]
fn every_listed_type_is_a_format_the_tool_draws_to_png() {
    let mimes: Vec<_> = key("MimeType").split(';').filter(|m| !m.is_empty()).collect();
    assert!(!mimes.is_empty());
    for mime in mimes {
        let format = format_for(mime).unwrap_or_else(|| panic!("`{mime}` is not a known suite type"));
        assert!(supported(format, Format::Png), "{mime} cannot be drawn to png yet");
        // Presentations are not drawn yet by the engine, so they stay out.
        assert_ne!(format.category(), Some(suite_convert::Category::Presentation), "{mime}");
    }
}

#[test]
fn the_command_parses_and_plans_with_the_placeholders_filled() {
    let exec = key("Exec").replace("%s", "128").replace("%i", "in.docx").replace("%o", "/tmp/out.png");
    let mut words = exec.split_whitespace();
    assert_eq!(words.next(), Some("suite-convert"));
    let Ok(Command::Convert(args)) = parse(words.map(str::to_owned)) else { panic!("did not parse: {exec}") };
    let planned = plan(&args).expect("plans");
    assert_eq!((planned.from, planned.to), (Format::Docx, Format::Png));
    assert_eq!((args.size, args.page), (128, 1));
    assert_eq!(key("TryExec"), "suite-convert");
}

/// Flatpak exports `share/thumbnailers` from the app that ships it, so exactly
/// one manifest installs the binary and the entry: Letters. Three apps
/// installing the same file would clash on the host.
#[test]
fn exactly_the_letters_manifest_ships_the_binary_and_the_entry() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../flatpak");
    let mut shippers = Vec::new();
    for app in ["letters", "tables", "decks"] {
        let text = std::fs::read_to_string(root.join(format!("org.tunaos.{app}.json"))).unwrap();
        let _: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        let ships_entry = text.contains("share/thumbnailers/org.tunaos.suite.thumbnailer");
        let ships_binary = text.contains("/app/bin/suite-convert") && text.contains("-p suite-convert");
        assert_eq!(ships_entry, ships_binary, "{app} ships the entry without the binary or the reverse");
        if ships_entry {
            shippers.push(app);
        }
    }
    assert_eq!(shippers, ["letters"]);
}
