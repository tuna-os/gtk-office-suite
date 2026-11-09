//! Reader View's tests.

use super::*;

#[test]
fn styles_scripts_and_attributes_are_gone() {
    let html = "<html><head><style>p{color:red}</style><script>alert(1)</script></head>\
                <body style=\"background:#000\"><p class=\"x\" style=\"color:red\" \
                onclick=\"go()\">Hello <span style=\"font-size:40px\">world</span></p></body></html>";
    let out = extract(html);
    assert_eq!(out, "<p>Hello world</p>");
}

#[test]
fn hidden_preview_text_is_dropped() {
    let html = "<div style=\"display:none;max-height:0;overflow:hidden\">Preview text you \
                should not see</div><div style=\"mso-hide:all\">Outlook only</div>\
                <div style=\"opacity:0\">ghost</div><div style=\"height:0;overflow:hidden\">pad</div>\
                <div hidden>h</div><p>The real message.</p>";
    let out = extract(html);
    assert_eq!(out, "<p>The real message.</p>");
}

#[test]
fn a_zero_font_size_column_is_not_hidden() {
    // MJML sets font-size:0px on every column and cell; the text inside
    // sets its own size and is very much meant to be read.
    let html = "<div style=\"font-size:0px;text-align:left;display:inline-block;width:100%\">\
                <table><tr><td style=\"font-size:0px;padding:10px 25px\"><div style=\"font-size:16px\">Convention reminder</div></td></tr></table></div>\
                <div style=\"max-height:0px\">not clipped, shown</div>";
    assert_eq!(
        extract(html),
        "<p>Convention reminder</p><p>not clipped, shown</p>"
    );
}

#[test]
fn a_heading_holds_a_line_not_boxes() {
    let html = "<h2><p>Your package</p><p>was delivered!</p></h2><p><div>a</div><div>b</div></p>";
    assert_eq!(
        extract(html),
        "<h2>Your package<br>was delivered!</h2><p>a</p><p>b</p>"
    );
}

#[test]
fn tracking_pixels_go_and_real_pictures_stay() {
    let html = "<p>Hi</p><img src=\"https://t.example/open.gif\" width=\"1\" height=\"1\">\
                <img src=\"https://x.example/a.png\" style=\"width:1px;height:1px\">\
                <img src=\"https://x.example/photo.jpg\" alt=\"A photo\" width=\"600\" \
                style=\"border:2px solid red\"><img src=\"cid:part1\"><img src=\"logo.png\">";
    let out = extract(html);
    assert_eq!(
        out,
        "<p>Hi</p><p><img src=\"https://x.example/photo.jpg\" alt=\"A photo\"><img src=\"cid:part1\"></p>"
    );
}

#[test]
fn a_layout_table_reads_as_blocks() {
    let html = "<table width=\"600\"><tr><td bgcolor=\"#fff\"><p>First</p></td></tr>\
                <tr><td><table><tr><td>Nested</td></tr></table></td></tr></table>";
    let out = extract(html);
    assert!(!out.contains("<table"), "{out}");
    assert_eq!(out, "<p>First</p><p>Nested</p>");
}

#[test]
fn a_data_table_keeps_its_grid() {
    let html = "<table><tr><th>Item</th><th>Price</th></tr><tr><td>Tea</td><td>3</td></tr></table>";
    let out = extract(html);
    assert_eq!(
        out,
        "<table><tr><th>Item</th><th>Price</th></tr><tr><td>Tea</td><td>3</td></tr></table>"
    );
    let plain = "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>";
    assert!(extract(plain).starts_with("<table>"), "{}", extract(plain));
}

#[test]
fn quote_markers_survive_for_the_fold() {
    let html = "<div>Reply text</div><div class=\"gmail_quote foo\">On Monday x wrote:\
                <blockquote type=\"cite\" style=\"margin:0\">Earlier</blockquote></div>";
    let out = extract(html);
    assert!(out.contains("<div class=\"gmail_quote\">"), "{out}");
    assert!(out.contains("<blockquote type=\"cite\">"), "{out}");
    assert!(!out.contains("foo"), "{out}");
}

#[test]
fn links_keep_their_target_and_nothing_dangerous() {
    let html = "<p><a href=\"https://a.example/x\" style=\"color:red\" target=\"_blank\">go</a> \
                <a href=\"javascript:alert(1)\">bad</a> <a href=\"#top\">top</a> \
                <a href=\"mailto:x@y.z\">mail</a></p>";
    let out = extract(html);
    assert_eq!(
        out,
        "<p><a href=\"https://a.example/x\">go</a> bad top <a href=\"mailto:x@y.z\">mail</a></p>"
    );
}

#[test]
fn plain_text_keeps_its_lines() {
    let html = "<html><body><div class=\"hylki-plain\">line one\n\n  indented\n&gt; quoted</div></body></html>";
    let out = extract(html);
    assert_eq!(
        out,
        "<div class=\"hylki-plain\">line one\n\n  indented\n&gt; quoted</div>"
    );
}

#[test]
fn raw_plain_text_keeps_its_lines_too() {
    let out = extract("Hi,\n\nTwo lines & a line\n  indented.\n");
    assert_eq!(
        out,
        "<div class=\"hylki-plain\">Hi,\n\nTwo lines &amp; a line\n  indented.</div>"
    );
    // The ordinary view's own plain wrapper (`body_html`) reads the same.
    let wrapped =
        "<!doctype html><html><head></head><body class=\"hylki-plain\">a\n\nb</body></html>";
    assert_eq!(extract(wrapped), "<div class=\"hylki-plain\">a\n\nb</div>");
}

#[test]
fn double_breaks_become_paragraphs() {
    let html = "<div>First line<br>still first<br><br>Second<br/>\n<br/>Third</div>";
    let out = extract(html);
    assert_eq!(
        out,
        "<p>First line<br>still first</p><p>Second</p><p>Third</p>"
    );
}

#[test]
fn empty_boxes_and_spacers_are_dropped() {
    let html =
        "<div>&nbsp;</div><div><div></div></div><p><br></p><table><tr><td>&nbsp;</td></tr></table>\
                <div><span> </span>Text</div>";
    let out = extract(html);
    assert_eq!(out, "<p>Text</p>");
}

#[test]
fn text_is_escaped_and_whitespace_collapsed() {
    let html = "<p>a &lt; b   &amp;\n\n c</p>";
    assert_eq!(extract(html), "<p>a &lt; b &amp; c</p>");
}

#[test]
fn inline_formatting_is_unified() {
    let html = "<p><b>bold</b> <i>it</i> <font color=\"red\">red</font> <tt>code</tt> <strike>no</strike></p>";
    assert_eq!(
        extract(html),
        "<p><strong>bold</strong> <em>it</em> red <code>code</code> <s>no</s></p>"
    );
}

#[test]
fn the_document_carries_the_sheet_and_theme() {
    let doc = render("<p>x</p>", true, "#3584e4");
    assert!(doc.contains("color-scheme:dark"), "{doc}");
    assert!(doc.contains("#3584e4"), "{doc}");
    assert!(doc.contains("<body><p>x</p></body>"), "{doc}");
    let light = render("<p>x</p>", false, "#3584e4");
    assert!(light.contains("color-scheme:light"), "{light}");
}

/// `HYLKI_READER_PROBE=<file> cargo test --bin hylki reader::tests::probe_file -- --ignored --nocapture`
/// prints what the reader makes of a cached body dumped to a file.
#[test]
#[ignore = "needs HYLKI_READER_PROBE (a live account or service); run by hand"]
fn probe_file() {
    let Ok(path) = std::env::var("HYLKI_READER_PROBE") else {
        return;
    };
    let body = std::fs::read_to_string(path).unwrap();
    println!("{}", extract(&body));
}

#[test]
fn nested_links_are_flattened() {
    let html = "<a href=\"https://a.example/\"><table><tr><td><a href=\"https://b.example/\">in</a></td></tr></table></a>";
    let out = extract(html);
    assert_eq!(out.matches("<a ").count(), 1, "{out}");
}
