//! "Report a Problem": a new-issue link on the suite's tracker with the
//! app, its version and the system already filled in, so a report says
//! which app and build it is about without the reporter having to know.

/// The suite's issue tracker; the same one each app's metainfo names as
/// its `bugtracker`.
pub const ISSUES_URL: &str = "https://github.com/tuna-os/gtk-office-suite/issues";

/// What a report is about: the app and the system it runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportContext {
    /// The app's display name, e.g. "Letters".
    pub app_name: String,
    /// The app's version, e.g. "2.1.0".
    pub version: String,
    /// The OS's `PRETTY_NAME`; inside Flatpak this is the runtime's.
    pub os: Option<String>,
    /// The toolkit versions, e.g. "GTK 4.16.3, libadwaita 1.6.2".
    pub toolkit: String,
}

/// The new-issue URL for a report from `ctx`: the title is prefixed with
/// the app's name and the body is a short template ending in the app,
/// version and system.
pub fn report_problem_url(ctx: &ReportContext) -> String {
    let title = format!("[{}] ", ctx.app_name);
    let body = format!(
        "**What happened?**\n\n\n\
         **What did you expect to happen?**\n\n\n\
         **Steps to reproduce**\n1. \n\n\
         ---\n\
         App: {} {}\n\
         System: {}\n\
         Toolkit: {}\n",
        ctx.app_name,
        ctx.version,
        ctx.os.as_deref().unwrap_or("unknown"),
        ctx.toolkit,
    );
    format!("{ISSUES_URL}/new?title={}&body={}", encode(&title), encode(&body))
}

/// The OS's `PRETTY_NAME` from os-release(5): `/etc/os-release`, else
/// `/usr/lib/os-release`.
pub fn os_pretty_name() -> Option<String> {
    ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| pretty_name(&text))
}

/// `PRETTY_NAME` from the text of an os-release(5) file, unquoted.
fn pretty_name(os_release: &str) -> Option<String> {
    let value = os_release.lines().find_map(|l| l.trim().strip_prefix("PRETTY_NAME="))?;
    let value = value.trim();
    let unquoted = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value);
    (!unquoted.is_empty()).then(|| unquoted.to_string())
}

/// Percent-encode `s` for a URL query value: everything but RFC 3986
/// unreserved characters, byte by byte, so non-ASCII text survives.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }

    fn query(url: &str, key: &str) -> String {
        let q = url.split_once('?').unwrap().1;
        let v = q.split('&').find_map(|kv| kv.strip_prefix(&format!("{key}="))).unwrap();
        decode(v)
    }

    fn ctx(os: Option<&str>) -> ReportContext {
        ReportContext {
            app_name: "Tables".into(),
            version: "2.1.0".into(),
            os: os.map(str::to_string),
            toolkit: "GTK 4.16.3, libadwaita 1.6.2".into(),
        }
    }

    /// The link opens a new issue on the suite's tracker, titled with the
    /// app, and its body names the app, version, system and toolkit.
    #[test]
    fn report_names_the_app_version_and_system() {
        let url = report_problem_url(&ctx(Some("Fedora Linux 41 (Workstation Edition)")));
        assert!(url.starts_with("https://github.com/tuna-os/gtk-office-suite/issues/new?"));
        assert_eq!(query(&url, "title"), "[Tables] ");
        let body = query(&url, "body");
        assert!(body.contains("App: Tables 2.1.0\n"), "{body}");
        assert!(body.contains("System: Fedora Linux 41 (Workstation Edition)\n"), "{body}");
        assert!(body.contains("Toolkit: GTK 4.16.3, libadwaita 1.6.2\n"), "{body}");
    }

    /// Characters that delimit a query (`&`, `=`, `#`, `+`, space,
    /// newline) and non-ASCII text cannot break the URL apart: the query
    /// has exactly the two fields and each decodes back to what was put in.
    #[test]
    fn query_delimiters_and_unicode_are_encoded() {
        let mut c = ctx(Some("Ünïcode & Co=1 #2 + x"));
        c.app_name = "Décks & Co".into();
        let url = report_problem_url(&c);
        let q = url.split_once('?').unwrap().1;
        assert_eq!(q.split('&').count(), 2, "{url}");
        assert!(!q.contains(' ') && !q.contains('\n') && !q.contains('#') && !q.contains('+'));
        assert_eq!(query(&url, "title"), "[Décks & Co] ");
        assert!(query(&url, "body").contains("System: Ünïcode & Co=1 #2 + x\n"));
    }

    #[test]
    fn unknown_system_is_said_to_be_unknown() {
        assert!(query(&report_problem_url(&ctx(None)), "body").contains("System: unknown\n"));
    }

    #[test]
    fn pretty_name_is_read_unquoted_from_os_release() {
        let text = "NAME=\"GNOME\"\nVERSION=\"47 (Flatpak runtime)\"\nPRETTY_NAME=\"GNOME 47 (Flatpak runtime)\"\nID=org.gnome.platform\n";
        assert_eq!(pretty_name(text).as_deref(), Some("GNOME 47 (Flatpak runtime)"));
        assert_eq!(pretty_name("PRETTY_NAME='Arch Linux'\n").as_deref(), Some("Arch Linux"));
        assert_eq!(pretty_name("PRETTY_NAME=Debian\n").as_deref(), Some("Debian"));
        assert_eq!(pretty_name("PRETTY_NAME=\"\"\n"), None);
        assert_eq!(pretty_name("NAME=Debian\n"), None);
    }
}
