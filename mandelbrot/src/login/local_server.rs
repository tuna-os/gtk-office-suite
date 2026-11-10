use gettextrs::gettext;
use gtk::gio;
use matrix_sdk::utils::local_server::{
    LocalServerBuilder, LocalServerRedirectHandle, LocalServerResponse,
};
use tracing::error;
use url::Url;

use crate::{APP_NAME, spawn_tokio};

/// The HTML template for the landing page.
const LOCAL_SERVER_LANDING_PAGE_TEMPLATE: &str = include_str!("local_server_landing_page.html");

/// Spawn a local server for listening to redirects.
pub(super) async fn spawn_local_server() -> Result<(Url, LocalServerRedirectHandle), ()> {
    spawn_tokio!(async move {
        LocalServerBuilder::new()
            .response(local_server_landing_page())
            .spawn()
            .await
    })
    .await
    .expect("task was not aborted")
    .map_err(|error| {
        error!("Could not spawn local server: {error}");
    })
}

/// The landing page, after the user performed the authentication and is
/// redirected to the local server.
fn local_server_landing_page() -> LocalServerResponse {
    let mut html = LOCAL_SERVER_LANDING_PAGE_TEMPLATE.to_owned();

    replace_html_variable(&mut html, "app_name", APP_NAME);
    replace_html_variable(&mut html, "title", &gettext("Authorization Completed"));
    replace_html_variable(
        &mut html,
        "message",
        &gettext(
            "The authorization step is complete. You can close this page and go back to Mandelbrot.",
        ),
    );
    replace_html_variable(&mut html, "icon", &svg_icon());

    LocalServerResponse::Html(html)
}

/// Replace the variable with the given name by the given value in the given
/// HTML.
///
/// The syntax for a variable is `@name@`. This is the same format as meson's
/// `configure_file` function.
///
/// Logs an error if the variable is not found.
fn replace_html_variable(html: &mut String, name: &str, value: &str) {
    let pattern = format!("@{name}@");

    // This is a programmer error.
    assert!(
        html.contains(&pattern),
        "Variable `{pattern}` should be present in HTML template"
    );

    *html = html.replace(&pattern, value);
}

/// Get the application SVG icon, ready to be embedded in HTML code.
///
/// Panics if the icon is not found or is invalid in some way.
fn svg_icon() -> String {
    // Load the icon from the application resources.
    let bytes = gio::resources_lookup_data(
        "/org/tunaos/mandelbrot/icons/scalable/apps/org.tunaos.mandelbrot.svg",
        gio::ResourceLookupFlags::NONE,
    )
    .expect("Application SVG icon should be present in GResources");

    // Convert the bytes to a string, since it should be SVG.
    let icon = String::from_utf8(bytes.to_vec())
        .expect("Application SVG icon content should be a UTF-8 string");

    // Remove the XML prologue, to inline the SVG directly into the HTML.
    icon.trim()
        .strip_prefix(r#"<?xml version="1.0" encoding="UTF-8"?>"#)
        .expect("Application SVG icon should start with an XML prologue")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use gtk::gio;
    use matrix_sdk::utils::local_server::LocalServerResponse;
    use strass::assert_let;

    use super::local_server_landing_page;
    use crate::config::tests::BUILD_DIR;

    #[gtk::test]
    fn generate_local_server_landing_page() {
        let resources_file = format!("{BUILD_DIR}/data/resources/resources.gresource");
        let res = gio::Resource::load(resources_file).expect("Could not load gresource file");
        gio::resources_register(&res);

        // Check that the variables were all replaced.
        assert_let!(LocalServerResponse::Html(html) = local_server_landing_page());
        assert!(!html.is_empty());
        assert!(!html.contains("@app_name@"));
        assert!(!html.contains("@title@"));
        assert!(!html.contains("@message@"));
        assert!(!html.contains("@icon@"));
    }
}
