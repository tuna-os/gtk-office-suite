use ruma::html::Html;

use super::inline_html::InlineHtmlBuilder;

#[test]
fn text_with_no_markup() {
    let html = Html::parse("A simple text");
    let (s, pills) = InlineHtmlBuilder::new(false, false, false).build_with_nodes(html.children());

    assert_eq!(s, "A simple text");
    assert!(pills.is_none());
}

#[test]
fn single_line() {
    let html = Html::parse("A simple text<br>on several lines");
    let (s, pills) = InlineHtmlBuilder::new(true, false, false).build_with_nodes(html.children());

    assert_eq!(s, "A simple text…");
    assert!(pills.is_none());

    let html = Html::parse("\nThis is a paragraph<br />\n\nThis is another paragraph\n");
    let (s, pills) = InlineHtmlBuilder::new(true, false, false).build_with_nodes(html.children());

    assert_eq!(s, "This is a paragraph…");
    assert!(pills.is_none());
}

#[test]
fn add_ellipsis() {
    let html = Html::parse("A simple text");
    let (s, pills) = InlineHtmlBuilder::new(false, true, false).build_with_nodes(html.children());

    assert_eq!(s, "A simple text…");
    assert!(pills.is_none());
}

#[test]
fn no_duplicate_ellipsis() {
    let html = Html::parse("A simple text...<br>...on several lines");
    let (s, pills) = InlineHtmlBuilder::new(true, false, false).build_with_nodes(html.children());

    assert_eq!(s, "A simple text...");
    assert!(pills.is_none());
}

#[test]
fn trim_end_spaces() {
    let html = Html::parse("A high-altitude text 🗻   ");
    let (s, pills) = InlineHtmlBuilder::new(false, false, false).build_with_nodes(html.children());

    assert_eq!(s, "A high-altitude text 🗻");
    assert!(pills.is_none());
}

#[test]
fn collapse_whitespace() {
    let original = "Hello \nyou! \nYou are <b>my \nfriend</b>.";
    let html = Html::parse(original);

    let (s, pills) = InlineHtmlBuilder::new(false, false, false).build_with_nodes(html.children());
    assert_eq!(s, "Hello you! You are <b>my friend</b>.");
    assert!(pills.is_none());

    let (s, pills) = InlineHtmlBuilder::new(false, false, true).build_with_nodes(html.children());
    assert_eq!(s, original);
    assert!(pills.is_none());

    let original = " Hello    \nyou! \n\nYou are \n<b>   my \nfriend   </b>.  ";
    let html = Html::parse(original);

    let (s, pills) = InlineHtmlBuilder::new(false, false, false).build_with_nodes(html.children());
    assert_eq!(s, "Hello you! You are <b>my friend</b>.");
    assert!(pills.is_none());

    let (s, pills) = InlineHtmlBuilder::new(false, false, true).build_with_nodes(html.children());
    assert_eq!(s, original);
    assert!(pills.is_none());
}

#[test]
fn sanitize_inline_html() {
    let html = Html::parse(
        r#"A <strong>text</strong> with <a href="https://docs.local/markup"><i>markup</i></a>"#,
    );
    let (s, pills) = InlineHtmlBuilder::new(false, false, false).build_with_nodes(html.children());

    assert_eq!(
        s,
        r#"A <b>text</b> with <a href="https://docs.local/markup" title="https://docs.local/markup"><i>markup</i></a>"#
    );
    assert!(pills.is_none());
}

#[test]
fn escape_markup() {
    let html = Html::parse(
        r#"Go to <a href="https://docs.local?this=this&that=that">this &amp; that docs</a>"#,
    );
    let (s, pills) = InlineHtmlBuilder::new(false, false, false).build_with_nodes(html.children());

    assert_eq!(
        s,
        r#"Go to <a href="https://docs.local?this=this&amp;that=that" title="https://docs.local?this=this&amp;amp;that=that">this &amp; that docs</a>"#
    );
    assert!(pills.is_none());
}

#[test]
fn linkify() {
    let html = Html::parse(
        "The homepage is https://gnome.org, and you can contact me at contact@me.local",
    );
    let (s, pills) = InlineHtmlBuilder::new(false, false, false).build_with_nodes(html.children());

    assert_eq!(
        s,
        r#"The homepage is <a href="https://gnome.org" title="https://gnome.org">https://gnome.org</a>, and you can contact me at <a href="mailto:contact@me.local" title="mailto:contact@me.local">contact@me.local</a>"#
    );
    assert!(pills.is_none());
}

#[test]
fn do_not_linkify_inside_anchor() {
    let html = Html::parse(r#"The homepage is <a href="https://gnome.org">https://gnome.org</a>"#);
    let (s, pills) = InlineHtmlBuilder::new(false, false, false).build_with_nodes(html.children());

    assert_eq!(
        s,
        r#"The homepage is <a href="https://gnome.org" title="https://gnome.org">https://gnome.org</a>"#
    );
    assert!(pills.is_none());
}

#[test]
fn do_not_linkify_inside_code() {
    let html = Html::parse("The homepage is <code>https://gnome.org</code>");
    let (s, pills) = InlineHtmlBuilder::new(false, false, false).build_with_nodes(html.children());

    assert_eq!(s, "The homepage is <tt>https://gnome.org</tt>");
    assert!(pills.is_none());
}

#[test]
fn emote_name() {
    let html = Html::parse("sent a beautiful picture.");
    let (s, pills) = InlineHtmlBuilder::new(false, false, false)
        .append_emote_with_name(&mut Some("Jun"))
        .build_with_nodes(html.children());

    assert_eq!(s, "<b>Jun</b> sent a beautiful picture.");
    assert!(pills.is_none());
}
