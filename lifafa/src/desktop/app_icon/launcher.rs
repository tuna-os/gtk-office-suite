//! The launcher: a copy of the desktop entry in the user's applications
//! directory pointing at the chosen icon, and the `mimeinfo.cache` beside
//! it that keeps Hylki the mail handler while the copy exists.

use std::path::PathBuf;

use super::{host_data_home, icon_is_ours};

/// Marker key in a launcher Hylki wrote, so only its own files are ever
/// removed or regenerated.
const LAUNCHER_MARK: &str = "X-Hylki-Icon-Launcher";

/// When the launcher was last written by this process.
static LAST_LAUNCHER_WRITE: std::sync::Mutex<Option<std::time::Instant>> =
    std::sync::Mutex::new(None);

/// How long the desktop takes to act on a rewritten launcher. GNOME Shell
/// rate-limits its watch on the applications directory and then waits a
/// further five seconds before reloading; only then does it replace the
/// app object a new window will be bound to. A restart that lands inside
/// that window binds the new window to the old object — and the dock
/// shows the old icon.
const LAUNCHER_SETTLE: std::time::Duration = std::time::Duration::from_secs(9);

fn note_launcher_written() {
    *LAST_LAUNCHER_WRITE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
}

/// How much longer to keep running before a restart, so the desktop has
/// acted on the launcher written for the current choice. Zero when it has
/// had the time, or nothing was written.
pub(crate) fn launcher_settle_remaining() -> std::time::Duration {
    let last = *LAST_LAUNCHER_WRITE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match last {
        Some(t) => LAUNCHER_SETTLE.saturating_sub(t.elapsed()),
        None => std::time::Duration::ZERO,
    }
}

/// How the app's launcher can be pointed at an icon. The desktop resolves
/// an app's icon through its `.desktop` entry, and a per-user copy of that
/// entry in `~/.local/share/applications` takes precedence over the
/// installed one — the mechanism menu editors use.
pub(super) enum Launcher {
    /// Flatpak and packaged installs: a copy of the installed entry,
    /// carrying the chosen icon and everything else verbatim, that hides
    /// itself once the app is gone (`TryExec`).
    Shadow {
        base: PathBuf,
        user_path: PathBuf,
        /// Already one of ours.
        ours: bool,
        /// The `Exec` line Flatpak exports (the sandbox copy's is bare).
        exec: Option<String>,
        try_exec: String,
        flatpak: bool,
    },
    /// A launcher the user's own install put there (install.sh, a source
    /// tree), or a copy the user made of the installed one (a menu editor
    /// writes one): only its `Icon=` line is ever touched.
    Owned(PathBuf),
    /// No installed launcher at all.
    None,
}

impl Launcher {
    pub(super) fn find() -> Self {
        let file = format!("{}.desktop", crate::APP_ID);
        let Some(user_dir) = host_data_home().map(|d| d.join("applications")) else {
            return Launcher::None;
        };
        let user_path = user_dir.join(&file);
        let ours = std::fs::read_to_string(&user_path)
            .map(|t| t.contains(LAUNCHER_MARK))
            .unwrap_or(false);

        // A copy without our mark is the user's (or their install's), under
        // the Flatpak too: rewriting it whole would lose whatever else they
        // changed in it.
        if user_path.exists() && !ours {
            return Launcher::Owned(user_path);
        }
        if crate::desktop::platform::is_flatpak() {
            let base = PathBuf::from("/app/share/applications").join(&file);
            let Some((exec, try_exec)) = flatpak_exec() else {
                return Launcher::None;
            };
            return Launcher::Shadow {
                base,
                user_path,
                ours,
                exec: Some(exec),
                try_exec,
                flatpak: true,
            };
        }
        // An AppImage installs nothing (#235): the only launcher is the one
        // the integrator wrote (AppManager, Gear Lever), under a name of its
        // own choosing and pointing at the bundle. Find it by what it runs
        // and touch its `Icon` line alone, as for any other entry we did not
        // write ourselves. Nothing to point at means nothing to do: an
        // un-integrated bundle has no launcher at all.
        if let Some(bundle) = crate::desktop::platform::appimage() {
            return match launcher_running(&user_dir, &bundle) {
                Some(path) => Launcher::Owned(path),
                None => Launcher::None,
            };
        }
        for dir in gtk::glib::system_data_dirs() {
            let base = dir.join("applications").join(&file);
            if !base.is_file() {
                continue;
            }
            // Hide the copy once the packaged binary is gone: the first
            // word of the installed Exec, as the desktop resolves it.
            let try_exec = std::fs::read_to_string(&base)
                .ok()
                .and_then(|t| {
                    desktop_value(&t, "Exec")
                        .map(|e| e.split_whitespace().next().unwrap_or("").to_string())
                })
                .unwrap_or_default();
            return Launcher::Shadow {
                base,
                user_path,
                ours,
                exec: None,
                try_exec,
                flatpak: false,
            };
        }
        Launcher::None
    }

    /// The launcher's `Icon=` value, when Hylki did not write it. A menu
    /// editor keeps our mark when it edits our copy, so the value is what
    /// is judged, not the mark.
    pub(super) fn custom_icon(&self) -> Option<String> {
        let path = match self {
            Launcher::Shadow { user_path, .. } => user_path,
            Launcher::Owned(path) => path,
            Launcher::None => return None,
        };
        let text = std::fs::read_to_string(path).ok()?;
        let icon = desktop_value(&text, "Icon")?;
        (!icon.is_empty() && !icon_is_ours(&icon)).then_some(icon)
    }

    /// Point the launcher at `icon` (an absolute path); `restore` instead
    /// puts the installed launcher back (the default, where a copy exists).
    pub(super) fn point_at(&self, icon: &str, restore: bool) {
        let placed = match self {
            Launcher::Shadow {
                base,
                user_path,
                ours,
                exec,
                try_exec,
                flatpak,
            } => write_shadow(
                base,
                user_path,
                icon,
                restore,
                *ours,
                exec.as_deref(),
                try_exec,
                *flatpak,
            )
            .then_some(user_path.as_path()),
            Launcher::Owned(path) => {
                edit_icon_line(path, icon);
                Some(path.as_path())
            }
            Launcher::None => None,
        };
        // A launcher in the user's directory only counts as a handler for
        // its MIME types once that directory's cache says so (#242).
        if let Some(dir) = placed.and_then(|p| p.parent()) {
            sync_mime_cache(dir);
        }
    }
}

/// The launcher in `dir` whose `Exec` runs `bundle`, if one is there.
fn launcher_running(dir: &std::path::Path, bundle: &std::path::Path) -> Option<PathBuf> {
    let bundle = bundle.to_str()?;
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        if path.extension()? != "desktop" {
            return None;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        desktop_value(&text, "Exec")?
            .contains(bundle)
            .then_some(path)
    })
}

/// The `Exec` line Flatpak exports for this app (from `/.flatpak-info`), and
/// the exported `bin` wrapper that exists exactly as long as it is installed.
fn flatpak_exec() -> Option<(String, String)> {
    let info = std::fs::read_to_string("/.flatpak-info").ok()?;
    let get = |key: &str| -> Option<String> {
        info.lines()
            .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
            .map(|v| v.trim().to_string())
    };
    let branch = get("branch")?;
    let arch = get("arch")?;
    let app_path = get("app-path")?;
    // …/flatpak/app/<id>/<arch>/<branch>/<commit>/files → …/flatpak
    let root = app_path
        .split_once(&format!("/app/{}/", crate::APP_ID))
        .map(|(root, _)| root.to_string())?;
    // `%U` as in the shipped entry: one launch for every selected file, so
    // "Send with Hylki" from Files attaches them all to the same message.
    let exec = format!(
        "flatpak run --branch={branch} --arch={arch} --command=hylki --file-forwarding {} @@u %U @@",
        crate::APP_ID
    );
    Some((exec, format!("{root}/exports/bin/{}", crate::APP_ID)))
}

/// One key's value from a desktop file's main group (not an action's).
fn desktop_value(text: &str, key: &str) -> Option<String> {
    crate::desktop::platform::keyfile_value(text, "Desktop Entry", key)
}

/// Returns whether the user's directory holds a copy of ours afterwards,
/// or just stopped holding one: either way its MIME cache is out of date.
#[allow(clippy::too_many_arguments)]
fn write_shadow(
    base: &std::path::Path,
    user_path: &std::path::Path,
    icon: &str,
    restore: bool,
    ours: bool,
    exec: Option<&str>,
    try_exec: &str,
    flatpak: bool,
) -> bool {
    if restore {
        if ours {
            match std::fs::remove_file(user_path) {
                Ok(()) => {
                    tracing::info!("removed {}", user_path.display());
                    note_launcher_written();
                }
                Err(e) => tracing::warn!("could not remove {}: {e}", user_path.display()),
            }
        }
        return ours;
    }
    let Ok(text) = std::fs::read_to_string(base) else {
        tracing::warn!("no launcher at {}", base.display());
        return false;
    };
    let out = shadow_text(&text, icon, exec, try_exec, flatpak);
    if std::fs::read_to_string(user_path)
        .map(|cur| cur == out)
        .unwrap_or(false)
    {
        return true;
    }
    if let Some(dir) = user_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::write(user_path, out) {
        Ok(()) => {
            tracing::info!("wrote {}", user_path.display());
            note_launcher_written();
            true
        }
        Err(e) => {
            tracing::warn!("could not write {}: {e}", user_path.display());
            false
        }
    }
}

/// The launcher copy's text: `base` with our icon, and, under Flatpak, the
/// host's command in place of the sandbox's. Only the main group's `Exec`
/// is the app's own command; a `[Desktop Action]` group (New Message, #269)
/// runs the app with arguments of its own, which the host command has to
/// carry. The keys added at the end of the main group stay in it: appended
/// after an action group they would belong to the action.
fn shadow_text(
    text: &str,
    icon: &str,
    exec: Option<&str>,
    try_exec: &str,
    flatpak: bool,
) -> String {
    let mut out = String::new();
    let mut main = false;
    let mut added = false;
    let add_keys = |out: &mut String| {
        if !try_exec.is_empty() {
            out.push_str(&format!("TryExec={try_exec}\n"));
        }
        if flatpak {
            out.push_str(&format!("X-Flatpak={}\n", crate::APP_ID));
        }
        out.push_str(&format!("{LAUNCHER_MARK}=1\n"));
    };
    for line in text.lines() {
        if line.starts_with('[') {
            if main && !added {
                // Before the blank line that ends the group, if it has one.
                let trimmed = out.trim_end_matches('\n').len();
                let tail = out.split_off(trimmed);
                out.push('\n');
                add_keys(&mut out);
                out.push_str(&tail[1.min(tail.len())..]);
                added = true;
            }
            main = line.trim() == "[Desktop Entry]";
        }
        if main && line.starts_with("Icon=") {
            out.push_str(&format!("Icon={icon}\n"));
        } else if main && line.starts_with("Exec=") && exec.is_some() {
            out.push_str(&format!("Exec={}\n", exec.unwrap_or("")));
        } else if !main && exec.is_some() && line.starts_with("Exec=") {
            let args = line["Exec=".len()..].split_once(' ').map_or("", |(_, a)| a);
            out.push_str(&format!("Exec={}\n", action_exec(exec.unwrap_or(""), args)));
        } else if main
            && (line.starts_with("TryExec=")
                || line.starts_with("X-Flatpak=")
                || line.starts_with(LAUNCHER_MARK))
        {
            // Re-added at the end of the group.
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !added {
        add_keys(&mut out);
    }
    out
}

/// The host command for a launcher action under Flatpak: the app's own
/// command (see [`flatpak_exec`]) with the action's arguments in place of
/// the forwarded files.
fn action_exec(exec: &str, args: &str) -> String {
    let base = exec
        .strip_suffix(" @@u %U @@")
        .unwrap_or(exec)
        .replace(" --file-forwarding", "");
    if args.is_empty() {
        base
    } else {
        format!("{base} {args}")
    }
}

/// Bring `<dir>/mimeinfo.cache` up to date with the entries in `dir`, as
/// `update-desktop-database` would.
///
/// GIO finds a MIME type's handlers through each applications directory's
/// own cache, and an entry in the user's directory masks every entry of
/// the same id in the directories after it. So from the moment the icon
/// chooser's copy of the launcher exists, the Flatpak export's cache no
/// longer speaks for `co.hyprlab.Hylki.desktop`: unless the user
/// directory's cache names it too, the desktop has no email client at all
/// (Settings shows "No Apps Available" for Mail, a browser offers only web
/// mail for a mailto: link, #242). Nothing on the host rebuilds that cache
/// for a file an app wrote, and the sandbox cannot run the host's
/// `update-desktop-database`, so the cache is written here in the tool's
/// own format: every `.desktop` in the directory and, with a `<sub>-`
/// prefix, its subdirectories, `Hidden=true` skipped, types and ids in
/// byte order. Left alone when it is already right.
pub(crate) fn sync_mime_cache(dir: &std::path::Path) {
    if !dir.is_dir() {
        return;
    }
    let cache = dir.join("mimeinfo.cache");
    let out = mime_cache_text(dir);
    if std::fs::read_to_string(&cache)
        .map(|cur| cur == out)
        .unwrap_or(false)
    {
        return;
    }
    let tmp = dir.join(format!(".mimeinfo.cache.{}", std::process::id()));
    let written = std::fs::write(&tmp, &out).and_then(|()| std::fs::rename(&tmp, &cache));
    match written {
        Ok(()) => tracing::info!("rebuilt {}", cache.display()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            tracing::warn!("could not rebuild {}: {e}", cache.display());
        }
    }
}

/// The `mimeinfo.cache` for `dir`, as `update-desktop-database` writes it.
fn mime_cache_text(dir: &std::path::Path) -> String {
    let mut map: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        Default::default();
    collect_mime_types(dir, "", &mut map);
    let mut out = String::from("[MIME Cache]\n");
    for (mime, ids) in &map {
        out.push_str(mime);
        out.push('=');
        for id in ids {
            out.push_str(id);
            out.push(';');
        }
        out.push('\n');
    }
    out
}

fn collect_mime_types(
    dir: &std::path::Path,
    prefix: &str,
    map: &mut std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            collect_mime_types(&path, &format!("{prefix}{name}-"), map);
            continue;
        }
        if !name.ends_with(".desktop") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let id = format!("{prefix}{name}");
        for mime in desktop_mime_types(&text) {
            map.entry(mime).or_default().insert(id.clone());
        }
    }
}

/// The `MimeType` list of a desktop entry's main group; empty for an entry
/// that has none or is `Hidden`.
fn desktop_mime_types(text: &str) -> Vec<String> {
    const MAIN: &str = "Desktop Entry";
    let Some(kf) = crate::desktop::platform::keyfile(text) else {
        return Vec::new();
    };
    if kf.boolean(MAIN, "Hidden").unwrap_or(false) {
        return Vec::new();
    }
    kf.string_list(MAIN, "MimeType")
        .map(|types| {
            types
                .iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Replace the `Icon=` line of a launcher the user's own install owns.
fn edit_icon_line(path: &std::path::Path, icon: &str) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let mut out = String::new();
    let mut seen = false;
    for line in text.lines() {
        if line.starts_with("Icon=") {
            out.push_str(&format!("Icon={icon}\n"));
            seen = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !seen || out == text {
        return;
    }
    match std::fs::write(path, out) {
        Ok(()) => note_launcher_written(),
        Err(e) => tracing::warn!("could not update {}: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED: &str =
        "[MIME Cache]\ntext/plain=a.desktop;sub-b.desktop;\nx-scheme-handler/mailto=a.desktop;\n";

    /// A launcher's actions have `Exec` lines of their own; only the main
    /// entry's counts, and a hidden entry claims no types.
    #[test]
    fn desktop_entries_are_read_from_the_main_group() {
        let entry = "[Desktop Action compose]\nExec=other --compose\n\n\
                     [Desktop Entry]\nExec=hylki %U\nMimeType=x-scheme-handler/mailto;message/rfc822;\n";
        assert_eq!(
            super::desktop_value(entry, "Exec").as_deref(),
            Some("hylki %U")
        );
        assert_eq!(
            super::desktop_mime_types(entry),
            ["x-scheme-handler/mailto", "message/rfc822"]
        );
        assert!(super::desktop_mime_types(&format!("{entry}Hidden=true\n")).is_empty());
    }

    /// The same fixture `update-desktop-database` was run over by hand:
    /// its output is what the expectation below is copied from.
    fn fixture(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hylki-mime-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(
            dir.join("a.desktop"),
            "[Desktop Entry]\nType=Application\nName=A\nMimeType=x-scheme-handler/mailto;text/plain;\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("sub/b.desktop"),
            "[Desktop Entry]\nName=B\nMimeType=text/plain\n[Desktop Action x]\nMimeType=image/png;\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("hidden.desktop"),
            "[Desktop Entry]\nName=H\nHidden=true\nMimeType=text/plain;\n",
        )
        .unwrap();
        std::fs::write(dir.join("nomime.desktop"), "[Desktop Entry]\nName=N\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "MimeType=text/plain;\n").unwrap();
        dir
    }

    #[test]
    fn cache_matches_update_desktop_database() {
        let dir = fixture("text");
        assert_eq!(mime_cache_text(&dir), EXPECTED);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_writes_once_and_leaves_a_right_cache_alone() {
        let dir = fixture("sync");
        let cache = dir.join("mimeinfo.cache");
        sync_mime_cache(&dir);
        assert_eq!(std::fs::read_to_string(&cache).unwrap(), EXPECTED);
        let stamp = std::fs::metadata(&cache).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        sync_mime_cache(&dir);
        assert_eq!(
            std::fs::metadata(&cache).unwrap().modified().unwrap(),
            stamp
        );
        // A missing directory is nothing to write into.
        sync_mime_cache(&dir.join("absent"));
        assert!(!dir.join("absent").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A launcher with an action (#269): the main group's command becomes
    /// the host's, the action's keeps its arguments, and the keys the copy
    /// adds stay in the main group.
    #[test]
    fn a_launcher_copy_keeps_its_actions() {
        let base = "[Desktop Entry]\nName=Hylki\nExec=hylki %U\nIcon=co.hyprlab.Hylki\nActions=new-message;\n\n[Desktop Action new-message]\nName=New Message\nExec=hylki mailto:\n";
        let host = format!("flatpak run --branch=stable --arch=x86_64 --command=hylki --file-forwarding {} @@u %U @@", crate::APP_ID);
        let out = shadow_text(base, "/icons/x.png", Some(&host), "/exports/bin/x", true);
        let (main, action) = out.split_once("[Desktop Action new-message]").unwrap();
        assert!(main.contains(&format!("Exec={host}\n")));
        assert!(main.contains("Icon=/icons/x.png\n"));
        assert!(main.contains("TryExec=/exports/bin/x\n"));
        assert!(main.contains(&format!("{LAUNCHER_MARK}=1\n")));
        assert!(
            main.ends_with("=1\n\n"),
            "the group keeps its blank line: {main:?}"
        );
        assert_eq!(
            action,
            format!("\nName=New Message\nExec=flatpak run --branch=stable --arch=x86_64 --command=hylki {} mailto:\n", crate::APP_ID)
        );
        // Outside Flatpak the action is left as it is.
        let out = shadow_text(base, "/icons/x.png", None, "/usr/bin/hylki", false);
        assert!(out.contains("Exec=hylki %U\n") && out.contains("Exec=hylki mailto:\n"));
        assert!(out
            .split_once("[Desktop Action")
            .unwrap()
            .0
            .contains("TryExec=/usr/bin/hylki"));
        // A launcher with no action still ends with the added keys.
        let out = shadow_text(
            "[Desktop Entry]\nName=Hylki\nExec=hylki %U\n",
            "i",
            None,
            "",
            false,
        );
        assert!(out.ends_with(&format!("{LAUNCHER_MARK}=1\n")));
    }

    #[test]
    fn a_launcher_edited_outside_hylki_reports_its_icon() {
        let dir = std::env::temp_dir().join(format!("hylki-custom-icon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("launcher.desktop");
        // A menu editor keeps our mark when it edits our copy.
        std::fs::write(
            &path,
            format!("[Desktop Entry]\nName=Hylki\nIcon=Papirus-mail\n{LAUNCHER_MARK}=1\n"),
        )
        .unwrap();
        assert_eq!(
            Launcher::Owned(path.clone()).custom_icon().as_deref(),
            Some("Papirus-mail")
        );
        std::fs::write(
            &path,
            format!("[Desktop Entry]\nName=Hylki\nIcon={}\n", crate::APP_ID),
        )
        .unwrap();
        assert_eq!(Launcher::Owned(path.clone()).custom_icon(), None);
        assert_eq!(Launcher::None.custom_icon(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole launcher copy, byte for byte: the keys Hylki adds go at
    /// the end of the main group, before its blank line, and an action's
    /// command keeps its own arguments.
    #[test]
    fn a_launcher_copy_is_written_exactly() {
        let base = "[Desktop Entry]\nName=Hylki\nExec=hylki %U\nIcon=co.hyprlab.Hylki\nActions=new-message;\n\n[Desktop Action new-message]\nName=New Message\nExec=hylki mailto:\n";
        let host = format!(
            "flatpak run --branch=stable --arch=x86_64 --command=hylki --file-forwarding {} @@u %U @@",
            crate::APP_ID
        );
        let id = crate::APP_ID;
        assert_eq!(
            shadow_text(base, "/icons/x.png", Some(&host), "/exports/bin/x", true),
            format!(
                "[Desktop Entry]\nName=Hylki\nExec={host}\nIcon=/icons/x.png\nActions=new-message;\nTryExec=/exports/bin/x\nX-Flatpak={id}\n{LAUNCHER_MARK}=1\n\n[Desktop Action new-message]\nName=New Message\nExec=flatpak run --branch=stable --arch=x86_64 --command=hylki {id} mailto:\n"
            )
        );
        // Keys from an earlier copy are dropped and written once; an action
        // group's own Icon and Exec are left alone without a host command.
        let old = format!("[Desktop Entry]\nTryExec=old\nIcon=a\nX-Flatpak=old\n{LAUNCHER_MARK}=1\nExec=hylki %U\n[Desktop Action x]\nIcon=b\nExec=hylki --x\n");
        assert_eq!(
            shadow_text(&old, "NEW", None, "", false),
            format!("[Desktop Entry]\nIcon=NEW\nExec=hylki %U\n{LAUNCHER_MARK}=1\n[Desktop Action x]\nIcon=b\nExec=hylki --x\n")
        );
    }

    #[test]
    fn an_action_runs_the_host_command_with_its_own_arguments() {
        assert_eq!(
            action_exec(
                "flatpak run --command=hylki --file-forwarding co.x @@u %U @@",
                "mailto:"
            ),
            "flatpak run --command=hylki co.x mailto:"
        );
        assert_eq!(action_exec("hylki", ""), "hylki");
        assert_eq!(action_exec("x --file-forwarding y", ""), "x y");
    }

    #[test]
    fn mime_types_come_from_the_main_group_unless_hidden() {
        assert_eq!(
            desktop_mime_types(
                "[Desktop Entry]\nName=H\nMimeType=x-scheme-handler/mailto; message/rfc822 ;;\n"
            ),
            vec!["x-scheme-handler/mailto", "message/rfc822"]
        );
        assert!(desktop_mime_types(
            "[Desktop Entry]\nHidden=true\nMimeType=x-scheme-handler/mailto;\n"
        )
        .is_empty());
        assert!(desktop_mime_types("[Desktop Entry]\nName=H\n").is_empty());
        assert!(desktop_mime_types("[Other]\nMimeType=a/b;\n").is_empty());
        assert!(desktop_mime_types("not a key file").is_empty());
    }
}
