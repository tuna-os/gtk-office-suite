use super::linkify;

#[test]
fn linkify_text() {
    // URLs with scheme.
    let text = "https://gitlab.gnome.org/World/fractal";
    assert_eq!(
        linkify(text),
        r#"<a href="https://gitlab.gnome.org/World/fractal" title="https://gitlab.gnome.org/World/fractal">https://gitlab.gnome.org/World/fractal</a>"#
    );

    let text = "https://matrix.to/#/!somewhere%3Aexample.org?via=elsewhere.ca";
    assert_eq!(
        linkify(text),
        r#"<a href="https://matrix.to/#/!somewhere%3Aexample.org?via=elsewhere.ca" title="https://matrix.to/#/!somewhere%3Aexample.org?via=elsewhere.ca">https://matrix.to/#/!somewhere%3Aexample.org?via=elsewhere.ca</a>"#
    );

    // Email.
    let text = "admin@matrix.org";
    assert_eq!(
        linkify(text),
        r#"<a href="mailto:admin@matrix.org" title="mailto:admin@matrix.org">admin@matrix.org</a>"#
    );

    // URLs without scheme.
    let text = "gnome.org";
    assert_eq!(
        linkify(text),
        r#"<a href="https://gnome.org" title="https://gnome.org">gnome.org</a>"#
    );

    let text = "gitlab.gnome.org/World/fractal";
    assert_eq!(
        linkify(text),
        r#"<a href="https://gitlab.gnome.org/World/fractal" title="https://gitlab.gnome.org/World/fractal">gitlab.gnome.org/World/fractal</a>"#
    );

    let text = "matrix.to/#/!somewhere%3Aexample.org?via=elsewhere.ca";
    assert_eq!(
        linkify(text),
        r#"<a href="https://matrix.to/#/!somewhere%3Aexample.org?via=elsewhere.ca" title="https://matrix.to/#/!somewhere%3Aexample.org?via=elsewhere.ca">matrix.to/#/!somewhere%3Aexample.org?via=elsewhere.ca</a>"#
    );

    // `matrix:` URIs.
    let text = "matrix:r/somewhere:example.org";
    assert_eq!(
        linkify(text),
        r#"<a href="matrix:r/somewhere:example.org" title="matrix:r/somewhere:example.org">matrix:r/somewhere:example.org</a>"#
    );

    let text = "matrix:roomid/somewhere:example.org?via=elsewhere.ca";
    assert_eq!(
        linkify(text),
        r#"<a href="matrix:roomid/somewhere:example.org?via=elsewhere.ca" title="matrix:roomid/somewhere:example.org?via=elsewhere.ca">matrix:roomid/somewhere:example.org?via=elsewhere.ca</a>"#
    );

    let text = "matrix:roomid/somewhere:example.org/e/event?via=elsewhere.ca";
    assert_eq!(
        linkify(text),
        r#"<a href="matrix:roomid/somewhere:example.org/e/event?via=elsewhere.ca" title="matrix:roomid/somewhere:example.org/e/event?via=elsewhere.ca">matrix:roomid/somewhere:example.org/e/event?via=elsewhere.ca</a>"#
    );

    let text = "matrix:u/alice:example.org?action=chat";
    assert_eq!(
        linkify(text),
        r#"<a href="matrix:u/alice:example.org?action=chat" title="matrix:u/alice:example.org?action=chat">matrix:u/alice:example.org?action=chat</a>"#
    );

    // Matrix identifiers.
    let text = "#somewhere:example.org";
    assert_eq!(
        linkify(text),
        r#"<a href="https://matrix.to/#/%23somewhere:example.org" title="https://matrix.to/#/%23somewhere:example.org">#somewhere:example.org</a>"#
    );

    let text = "!somewhere:example.org";
    assert_eq!(
        linkify(text),
        r#"<a href="https://matrix.to/#/!somewhere:example.org" title="https://matrix.to/#/!somewhere:example.org">!somewhere:example.org</a>"#
    );

    let text = "@someone:example.org";
    assert_eq!(
        linkify(text),
        r#"<a href="https://matrix.to/#/@someone:example.org" title="https://matrix.to/#/@someone:example.org">@someone:example.org</a>"#
    );

    // Invalid TLDs.
    let text = "gnome.invalid";
    assert_eq!(linkify(text), "gnome.invalid");

    let text = "org.gnome.fractal";
    assert_eq!(linkify(text), "org.gnome.fractal");

    // `matrix:` that is not a URI scheme.
    let text = "My homeserver for matrix: gnome.org";
    assert_eq!(
        linkify(text),
        r#"My homeserver for matrix: <a href="https://gnome.org" title="https://gnome.org">gnome.org</a>"#
    );
}
