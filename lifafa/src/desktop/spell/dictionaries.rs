//! Which hunspell dictionaries are installed, and which ones checking runs
//! with: the configured languages, else the session locale's.

/// The dictionaries checking actually runs with: the configured languages,
/// else the session locale, mapped onto the installed dictionaries.
pub(crate) fn resolved_spell_languages() -> Vec<String> {
    let configured = crate::config::load_privacy().spellcheck_langs;
    super::resolve_languages(&configured, locale_language(), &installed_dictionaries())
}

/// The dictionaries "System language" stands for: the session locale's.
pub(crate) fn system_spell_languages() -> Vec<String> {
    super::resolve_languages("", locale_language(), &installed_dictionaries())
}

/// The session locale as a dictionary-style code ("en_US.UTF-8" → "en_US").
fn locale_language() -> Option<String> {
    crate::i18n::locale_env(false)
        .map(|l| l.split('.').next().unwrap_or_default().to_string())
        .find(|l| !l.is_empty() && l != "C" && l != "POSIX")
}

/// The hunspell dictionaries that resolve to real files. Inside the Flatpak
/// most of `/usr/share/hunspell` is dangling symlinks until the locale
/// extension carries the language; `metadata()` follows symlinks, so a
/// dangling one errs out and is rightly skipped.
pub(crate) fn installed_dictionaries() -> Vec<String> {
    let mut codes: std::collections::BTreeSet<String> = Default::default();
    // Exactly the places enchant's hunspell provider reads (verified
    // empirically — a listed dictionary the checker can't use is the silent
    // failure this feature avoids): the classic locations, every SYSTEM XDG
    // data dir (which is how the Flatpak's bundled /app/share/hunspell set
    // is found), and enchant's per-user drop-in under the config dir. The
    // user DATA dir is deliberately absent: enchant does not search it.
    let mut dirs = vec![
        std::path::PathBuf::from("/usr/share/hunspell"),
        std::path::PathBuf::from("/usr/share/myspell"),
    ];
    for d in gtk::glib::system_data_dirs() {
        dirs.push(d.join("hunspell"));
    }
    dirs.push(
        gtk::glib::user_config_dir()
            .join("enchant")
            .join("hunspell"),
    );
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let is_dic = p.extension().is_some_and(|x| x == "dic");
            if is_dic && p.metadata().map(|m| m.is_file()).unwrap_or(false) {
                if let Some(s) = p.file_stem().and_then(|s| s.to_str()) {
                    codes.insert(s.to_string());
                }
            }
        }
    }
    // The runtime ships English many times over (eighteen region variants,
    // all hardlinks to the same data); the list keeps the five anyone looks
    // for. Other languages are shown as installed.
    const KEPT_ENGLISH: &[&str] = &["en_US", "en_GB", "en_CA", "en_AU", "en_NZ"];
    codes
        .into_iter()
        .filter(|c| c.split('_').next() != Some("en") || KEPT_ENGLISH.contains(&c.as_str()))
        .collect()
}

/// Dictionary codes as the names people read, joined: "Deutsch
/// (Deutschland), Français (France)".
pub(crate) fn spell_language_names(codes: &[String]) -> String {
    codes
        .iter()
        .map(|c| super::language_display_name(c))
        .collect::<Vec<_>>()
        .join(", ")
}
