//! The release notes in the About window: this release's section of
//! docs/RELEASE_NOTES.md, rendered from its Markdown into GTK labels.

use adw::prelude::*;

use crate::i18n::i18n;

/// This release's section of `RELEASE_NOTES.md` — the same text the GitHub
/// release carries — headed by the version.
fn current_release_notes() -> String {
    let md = include_str!("../../docs/RELEASE_NOTES.md");
    let head = format!("## What's new in {}", crate::VERSION);
    let mut out = String::new();
    let mut on = false;
    for line in md.lines() {
        if line.starts_with("## ") {
            if on {
                break;
            }
            on = line == head || line.starts_with(&format!("{head} "));
            if on {
                out.push_str(&format!("## {}\n", crate::VERSION));
            }
            continue;
        }
        if on {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Where the whole release history lives.
const RELEASE_NOTES_URL: &str = "https://github.com/hyprlab/hylki/blob/main/docs/RELEASE_NOTES.md";

/// The About window's "Release Notes" page: this release's notes, with the
/// full history a button away on GitHub.
pub(super) fn release_notes_page() -> adw::NavigationPage {
    notes_page(
        "Release Notes",
        "notes",
        &current_release_notes(),
        Some((i18n("Full Release Notes on GitHub"), RELEASE_NOTES_URL)),
    )
}

/// Build a scrollable About sub-page from Markdown for the navigation stack,
/// reachable by `tag`. Pushed pages get a back button and slide animation from
/// the parent `NavigationView`.
fn notes_page(
    title: &str,
    tag: &str,
    md: &str,
    link: Option<(String, &'static str)>,
) -> adw::NavigationPage {
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let clamp = adw::Clamp::builder().maximum_size(460).build();

    let column = md_column(md);
    column.set_margin_top(18);
    column.set_margin_bottom(24);
    column.set_margin_start(18);
    column.set_margin_end(18);
    // A button at the foot, for the rest of the story elsewhere.
    if let Some((label, url)) = link {
        let button = gtk::Button::with_label(&label);
        button.add_css_class("pill");
        button.add_css_class("suggested-action");
        button.set_halign(gtk::Align::Center);
        button.set_margin_top(18);
        button.connect_clicked(move |b| {
            let parent = b.root().and_then(|r| r.downcast::<gtk::Window>().ok());
            crate::ui::launch::open_link(url, parent.as_ref());
        });
        column.append(&button);
    }

    clamp.set_child(Some(&column));
    scroller.set_child(Some(&clamp));

    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&scroller));

    adw::NavigationPage::builder()
        .title(title)
        .tag(tag)
        .child(&tv)
        .build()
}

/// Render Markdown as a column of widgets rather than one long label: a bullet
/// gets its own column so wrapped lines align under the text instead of running
/// back under the bullet, and each heading carries its own spacing. Handles
/// headings, bullets (nested one level), indented continuation paragraphs, and
/// the inline syntax in [`md_inline`].
fn md_column(md: &str) -> gtk::Box {
    /// A logical Markdown block, with source lines coalesced — CHANGELOG.md
    /// is hard-wrapped at ~72 columns, and rendering each source line as its
    /// own label left a ragged right edge instead of flowing text.
    enum Block {
        H2(String),
        H3(String),
        Bullet { nested: bool, text: String },
        Para { indented: bool, text: String },
    }

    let mut blocks: Vec<Block> = Vec::new();
    let mut cur: Option<Block> = None;
    let flush = |cur: &mut Option<Block>, blocks: &mut Vec<Block>| {
        if let Some(b) = cur.take() {
            blocks.push(b);
        }
    };
    for raw in md.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.is_empty() {
            flush(&mut cur, &mut blocks);
        } else if trimmed.starts_with("# ") {
            // The page's header bar already shows the document's title.
            flush(&mut cur, &mut blocks);
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            flush(&mut cur, &mut blocks);
            blocks.push(Block::H2(rest.to_string()));
        } else if let Some(rest) = trimmed.strip_prefix("### ") {
            flush(&mut cur, &mut blocks);
            blocks.push(Block::H3(rest.to_string()));
        } else if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            flush(&mut cur, &mut blocks);
            cur = Some(Block::Bullet {
                nested: indent >= 2,
                text: rest.to_string(),
            });
        } else {
            // A plain line continues the open bullet/paragraph, or starts one.
            match &mut cur {
                Some(Block::Bullet { text, .. }) | Some(Block::Para { text, .. }) => {
                    text.push(' ');
                    text.push_str(trimmed);
                }
                _ => {
                    cur = Some(Block::Para {
                        indented: indent >= 2,
                        text: trimmed.to_string(),
                    })
                }
            }
        }
    }
    flush(&mut cur, &mut blocks);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let mut first = true;
    // Anything before the first section heading is document front-matter
    // (RELEASE_NOTES' intro paragraph), not notes — skip it.
    let mut seen_section = false;
    for block in &blocks {
        if !seen_section {
            match block {
                Block::H2(_) | Block::H3(_) => seen_section = true,
                _ => continue,
            }
        }
        let widget: gtk::Widget = match block {
            Block::H3(text) => {
                let label = md_label(text, &["heading"]);
                label.set_margin_top(if first { 0 } else { 14 });
                label.into()
            }
            Block::H2(text) => {
                let label = md_label(text, &["title-4"]);
                label.set_margin_top(if first { 0 } else { 22 });
                label.set_margin_bottom(2);
                label.into()
            }
            Block::Bullet { nested, text } => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                row.set_margin_top(6);
                row.set_margin_start(if *nested { 18 } else { 0 });
                let bullet = gtk::Label::new(Some(if *nested { "\u{25e6}" } else { "\u{2022}" }));
                bullet.set_valign(gtk::Align::Start);
                bullet.add_css_class("dim-label");
                row.append(&bullet);
                let label = md_label(text, &[]);
                label.set_hexpand(true);
                row.append(&label);
                row.into()
            }
            Block::Para { indented, text } => {
                let label = md_label(text, &[]);
                label.set_margin_top(8);
                label.set_margin_start(if *indented { 26 } else { 0 });
                label.into()
            }
        };
        column.append(&widget);
        first = false;
    }
    column
}

/// A wrapped label carrying inline Markdown. Links are opened through the app's
/// own URI handler rather than GTK's default, which has no portal under Flatpak.
fn md_label(text: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_markup(&md_inline(text));
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    label.set_halign(gtk::Align::Start);
    for class in classes {
        label.add_css_class(class);
    }
    label.connect_activate_link(|_, uri| {
        crate::services::oauth::open_uri(uri);
        gtk::glib::Propagation::Stop
    });
    label
}

/// Inline Markdown → Pango markup: `**bold**`, `*italic*`, `` `code` `` and
/// `[text](url)`. Everything outside a marker is escaped, so the source may
/// contain `&` or `<` safely. Emphasis nests (a bold link, a link with code in
/// its text); an unclosed marker stays the literal character it is.
fn md_inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let joined = |from: usize, to: usize| -> String { chars[from..to].iter().collect() };
    // Where `needle` next starts, at or after `from`.
    let find = |from: usize, needle: &[char]| -> Option<usize> {
        if needle.len() > chars.len() {
            return None;
        }
        (from..=chars.len() - needle.len()).find(|&i| chars[i..i + needle.len()] == *needle)
    };

    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // Code spans first: their contents are literal, never re-parsed.
        if chars[i] == '`' {
            if let Some(end) = find(i + 1, &['`']) {
                out.push_str("<tt>");
                out.push_str(&gtk::glib::markup_escape_text(&joined(i + 1, end)));
                out.push_str("</tt>");
                i = end + 1;
                continue;
            }
        } else if chars[i..].starts_with(&['*', '*']) {
            if let Some(end) = find(i + 2, &['*', '*']) {
                out.push_str("<b>");
                out.push_str(&md_inline(&joined(i + 2, end)));
                out.push_str("</b>");
                i = end + 2;
                continue;
            }
        } else if chars[i] == '*' {
            if let Some(end) = find(i + 1, &['*']) {
                out.push_str("<i>");
                out.push_str(&md_inline(&joined(i + 1, end)));
                out.push_str("</i>");
                i = end + 1;
                continue;
            }
        } else if chars[i] == '[' {
            if let Some(close) = find(i + 1, &[']']) {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(paren) = find(close + 2, &[')']) {
                        out.push_str(&format!(
                            "<a href=\"{}\">{}</a>",
                            gtk::glib::markup_escape_text(&joined(close + 2, paren)),
                            md_inline(&joined(i + 1, close)),
                        ));
                        i = paren + 1;
                        continue;
                    }
                }
            }
        }
        out.push_str(&gtk::glib::markup_escape_text(&chars[i].to_string()));
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md_inline_renders_the_syntax_the_changelog_uses() {
        assert_eq!(md_inline("**bold**"), "<b>bold</b>");
        assert_eq!(md_inline("routed *every* port"), "routed <i>every</i> port");
        assert_eq!(md_inline("`set_visible`"), "<tt>set_visible</tt>");
        assert_eq!(
            md_inline("[Chris](https://github.com/chrispouliot)"),
            "<a href=\"https://github.com/chrispouliot\">Chris</a>"
        );
        // Emphasis nests, as in the changelog's bolded contributor links.
        assert_eq!(
            md_inline("**[Chris](https://example.com)**"),
            "<b><a href=\"https://example.com\">Chris</a></b>"
        );
    }

    #[test]
    fn md_inline_escapes_markup_and_keeps_stray_markers() {
        // Pango would refuse the whole label if these went through unescaped.
        assert_eq!(md_inline("a < b & c"), "a &lt; b &amp; c");
        // A code span's contents are literal, never re-parsed as Markdown.
        assert_eq!(md_inline("`a <b> *c*`"), "<tt>a &lt;b&gt; *c*</tt>");
        // Unclosed markers stay the characters they are rather than eating the line.
        assert_eq!(md_inline("2 * 3 = 6"), "2 * 3 = 6");
        assert_eq!(md_inline("see [the docs"), "see [the docs");
    }

    #[test]
    fn current_release_has_notes() {
        let notes = current_release_notes();
        assert!(
            notes.lines().count() > 2,
            "RELEASE_NOTES.md has no section for {}",
            crate::VERSION
        );
        assert!(notes.starts_with(&format!("## {}\n", crate::VERSION)));
    }

    #[test]
    fn changelog_and_release_notes_render_without_pango_errors() {
        // Pango refuses to render a label whose markup is malformed, blanking the
        // whole page — so every line of both documents has to parse. Link tags are
        // GtkLabel's own extension and are not known to `parse_markup`, so they
        // are lifted out first (which also asserts each one is closed).
        fn without_links(markup: &str) -> String {
            let mut out = String::new();
            let mut rest = markup;
            while let Some(open) = rest.find("<a href=\"") {
                out.push_str(&rest[..open]);
                let tail = &rest[open..];
                let close = tail.find('>').expect("link tag is closed");
                rest = &tail[close + 1..];
            }
            out.push_str(rest);
            out.replace("</a>", "")
        }

        for md in [
            include_str!("../../CHANGELOG.md"),
            include_str!("../../docs/RELEASE_NOTES.md"),
        ] {
            for line in md.lines() {
                let markup = md_inline(line);
                assert!(
                    gtk::pango::parse_markup(&without_links(&markup), '\0').is_ok(),
                    "does not parse as Pango markup: {line}\n  -> {markup}"
                );
            }
        }
    }
}
