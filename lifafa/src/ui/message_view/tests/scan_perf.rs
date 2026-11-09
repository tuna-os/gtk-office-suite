//! A timing check on the remote-content scanner, ignored by default.

use super::*;

#[test]
#[ignore = "timing-sensitive"]
fn scanning_a_large_message_is_quick() {
    // A big marketing email: lots of tags, a large stylesheet, many images.
    let mut html = String::from("<html><head><style>");
    for i in 0..2000 {
        html.push_str(&format!(
            ".c{i}{{background:url(https://cdn.example/bg{i}.png);color:#333}}"
        ));
    }
    html.push_str("</style></head><body>");
    for i in 0..5000 {
        html.push_str(&format!(
            "<div class=\"c{i}\" style=\"padding:2px\"><img src=\"https://cdn.example/i{i}.png\"><a href=\"https://example.com/{i}\">x</a></div>"
        ));
    }
    html.push_str("</body></html>");
    eprintln!("body is {} KB", html.len() / 1024);

    let t = std::time::Instant::now();
    assert!(has_remote_resources(&html));
    let detect = t.elapsed();
    let t = std::time::Instant::now();
    let stripped = strip_remote(&html);
    let strip = t.elapsed();
    eprintln!("detect {detect:?}, strip {strip:?}");
    assert!(!stripped.contains("cdn.example"));
    // Links are left alone.
    assert!(stripped.contains("https://example.com/4999"));
    assert!(detect.as_millis() < 250, "detection took {detect:?}");
    assert!(strip.as_millis() < 250, "stripping took {strip:?}");
}
