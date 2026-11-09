//! Subject-line spell checking (#114): the same enchant engine WebKit uses
//! for the message body, reached directly — GTK entries have no checker of
//! their own, and libspelling isn't in the runtime.
//!
//! enchant is dlopen'd at runtime rather than linked: it is guaranteed
//! present wherever WebKitGTK is (the GNOME runtime included), but linking
//! at build time would demand its -devel package on every build machine.
//! If the library or the dictionary is missing, checking simply reports
//! nothing misspelled — same silence the body would show.

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::rc::Rc;

mod dictionaries;

pub(crate) use dictionaries::{
    installed_dictionaries, resolved_spell_languages, spell_language_names, system_spell_languages,
};

extern "C" {
    // From glibc itself (libdl merged into libc since 2.34) — no crate, no
    // link flag needed beyond what every binary already has.
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

type BrokerInit = unsafe extern "C" fn() -> *mut c_void;
type RequestDict = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type DictCheck = unsafe extern "C" fn(*mut c_void, *const c_char, isize) -> c_int;
type DictAdd = unsafe extern "C" fn(*mut c_void, *const c_char, isize);

struct SpellChecker {
    dict: *mut c_void,
    check: DictCheck,
    add: DictAdd,
}

impl SpellChecker {
    /// A checker for `lang`, or `None` when enchant or the dictionary is
    /// unavailable. The broker and dictionary are deliberately never freed:
    /// one per language per process, alive for its whole life.
    fn new(lang: &str) -> Option<SpellChecker> {
        unsafe {
            let lib = dlopen(c"libenchant-2.so.2".as_ptr(), 1 /* RTLD_LAZY */);
            if lib.is_null() {
                return None;
            }
            let sym = |name: &CStr| dlsym(lib, name.as_ptr());
            let init = sym(c"enchant_broker_init");
            let req = sym(c"enchant_broker_request_dict");
            let check = sym(c"enchant_dict_check");
            let add = sym(c"enchant_dict_add");
            if init.is_null() || req.is_null() || check.is_null() || add.is_null() {
                return None;
            }
            let init: BrokerInit = std::mem::transmute(init);
            let req: RequestDict = std::mem::transmute(req);
            let check: DictCheck = std::mem::transmute(check);
            let add: DictAdd = std::mem::transmute(add);
            let broker = init();
            if broker.is_null() {
                return None;
            }
            let tag = CString::new(lang).ok()?;
            let dict = req(broker, tag.as_ptr());
            if dict.is_null() {
                return None;
            }
            Some(SpellChecker { dict, check, add })
        }
    }

    /// Whether enchant knows no such word. Errors (and interior NULs) count
    /// as correctly spelled — a checker must never cry wolf.
    fn is_misspelled(&self, word: &str) -> bool {
        let Ok(w) = CString::new(word) else {
            return false;
        };
        unsafe { (self.check)(self.dict, w.as_ptr(), word.len() as isize) > 0 }
    }

    /// Teach enchant a word: it joins the personal word list on disk (the
    /// same file WebKit's Learn Spelling writes) and stops being flagged by
    /// this checker at once.
    fn learn(&self, word: &str) {
        let Ok(w) = CString::new(word) else { return };
        unsafe { (self.add)(self.dict, w.as_ptr(), word.len() as isize) }
    }
}

thread_local! {
    /// One checker per language, built the first time the language is
    /// asked for, so switching languages back and forth builds nothing
    /// twice. A failed build is remembered too (`None`): a missing
    /// dictionary is not looked for again on every keystroke.
    static CHECKERS: RefCell<std::collections::HashMap<String, Option<Rc<SpellChecker>>>> =
        RefCell::new(Default::default());
}

/// The checkers for `langs` that could be built, in the same order.
fn checkers_for(langs: &[String]) -> Vec<Rc<SpellChecker>> {
    CHECKERS.with(|c| {
        let mut map = c.borrow_mut();
        langs
            .iter()
            .filter_map(|lang| {
                map.entry(lang.clone())
                    .or_insert_with(|| SpellChecker::new(lang).map(Rc::new))
                    .clone()
            })
            .collect()
    })
}

/// Forget every built checker, so the next check reloads the personal word
/// lists from disk.
fn reset_checkers() {
    CHECKERS.with(|c| c.borrow_mut().clear());
}

/// Whether `word` is misspelled under several dictionaries at once (#365):
/// only when every one of them rejects it. A message that mixes two
/// languages has each word in one of them; no checker at all flags nothing.
fn misspelled_in_all<C>(checkers: &[C], word: &str, rejects: impl Fn(&C, &str) -> bool) -> bool {
    !checkers.is_empty() && checkers.iter().all(|c| rejects(c, word))
}

/// The language codes a spelling setting lists, in order and each once.
/// Commas are the separator the app writes; semicolons and spaces are read
/// too, for a hand-edited file.
pub(crate) fn parse_languages(setting: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for code in setting
        .split([',', ';', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !out.iter().any(|c| c == code) {
            out.push(code.to_string());
        }
    }
    out
}

/// The setting as it is written back: "en_US, de_DE".
fn join_languages(codes: &[String]) -> String {
    codes.join(", ")
}

/// The setting after one language is switched on or off (#365), the same
/// way from Settings and from the composer's menu. `code` is empty for the
/// system language. Choosing a dictionary leaves the system language for
/// the list, and switching the last one off returns to it. Switching the
/// system language off keeps checking in `system`, the dictionaries it was
/// using, since checking in no language at all is not a choice.
pub(crate) fn toggle_language(setting: &str, code: &str, on: bool, system: &[String]) -> String {
    let mut codes = parse_languages(setting);
    if code.is_empty() {
        return match (on, codes.is_empty()) {
            (true, _) => String::new(),
            (false, true) => join_languages(system),
            (false, false) => join_languages(&codes),
        };
    }
    if on {
        if !codes.iter().any(|c| c == code) {
            codes.push(code.to_string());
        }
    } else {
        codes.retain(|c| c != code);
    }
    join_languages(&codes)
}

/// The dictionaries checking runs with for `setting` (#365): each listed
/// language, or `locale` when none is, mapped onto the installed
/// dictionaries in `dicts`. A language without its exact dictionary takes a
/// same-language one (en_GB for en_US); one with neither is left out, since
/// a missing dictionary checks nothing. When nothing is left, any
/// dictionary beats none. With no dictionaries visible at all the codes are
/// passed on as they are.
fn resolve_languages(setting: &str, locale: Option<String>, dicts: &[String]) -> Vec<String> {
    let mut wanted = parse_languages(setting);
    if wanted.is_empty() {
        wanted.push(locale.unwrap_or_else(|| "en_US".to_string()));
    }
    if dicts.is_empty() {
        return wanted;
    }
    let family = |c: &str| c.split(['_', '-']).next().unwrap_or(c).to_string();
    let mut out: Vec<String> = Vec::new();
    for want in &wanted {
        let found = dicts
            .iter()
            .find(|d| *d == want)
            .or_else(|| dicts.iter().find(|d| family(d) == family(want)));
        if let Some(d) = found {
            if !out.contains(d) {
                out.push(d.clone());
            }
        }
    }
    if out.is_empty() {
        out.extend(dicts.first().cloned());
    }
    out
}

/// Dictionary codes people actually encounter, named in the language itself
/// — a Russian speaker looking for their language finds "Русский", not a
/// code. Covers the runtime's English variants, the languages the Flatpak
/// bundles, and the common locale-extension arrivals.
const LANGUAGE_NAMES: &[(&str, &str)] = &[
    ("de_AT", "Deutsch (Österreich)"),
    ("de_CH", "Deutsch (Schweiz)"),
    ("de_DE", "Deutsch (Deutschland)"),
    ("en_AG", "English (Antigua and Barbuda)"),
    ("en_AU", "English (Australia)"),
    ("en_BS", "English (Bahamas)"),
    ("en_BW", "English (Botswana)"),
    ("en_BZ", "English (Belize)"),
    ("en_CA", "English (Canada)"),
    ("en_DK", "English (Denmark)"),
    ("en_GB", "English (United Kingdom)"),
    ("en_GH", "English (Ghana)"),
    ("en_HK", "English (Hong Kong)"),
    ("en_IE", "English (Ireland)"),
    ("en_IN", "English (India)"),
    ("en_JM", "English (Jamaica)"),
    ("en_MW", "English (Malawi)"),
    ("en_NA", "English (Namibia)"),
    ("en_NG", "English (Nigeria)"),
    ("en_NZ", "English (New Zealand)"),
    ("en_PH", "English (Philippines)"),
    ("en_SG", "English (Singapore)"),
    ("en_TT", "English (Trinidad and Tobago)"),
    ("en_US", "English (United States)"),
    ("en_ZA", "English (South Africa)"),
    ("en_ZM", "English (Zambia)"),
    ("en_ZW", "English (Zimbabwe)"),
    ("es_AR", "Español (Argentina)"),
    ("es_ES", "Español (España)"),
    ("es_MX", "Español (México)"),
    ("fr_BE", "Français (Belgique)"),
    ("fr_CA", "Français (Canada)"),
    ("fr_CH", "Français (Suisse)"),
    ("fr_FR", "Français (France)"),
    ("it_IT", "Italiano (Italia)"),
    ("nl_BE", "Nederlands (België)"),
    ("nl_NL", "Nederlands (Nederland)"),
    ("pl_PL", "Polski (Polska)"),
    ("pt_BR", "Português (Brasil)"),
    ("pt_PT", "Português (Portugal)"),
    ("ru_RU", "Русский (Россия)"),
    ("sv_FI", "Svenska (Finland)"),
    ("sv_SE", "Svenska (Sverige)"),
    ("uk_UA", "Українська (Україна)"),
];

/// Language-only endonyms, for codes the exact table doesn't carry.
const LANGUAGE_FAMILY_NAMES: &[(&str, &str)] = &[
    ("af", "Afrikaans"),
    ("ar", "العربية"),
    ("be", "Беларуская"),
    ("bg", "Български"),
    ("bn", "বাংলা"),
    ("ca", "Català"),
    ("cs", "Čeština"),
    ("cy", "Cymraeg"),
    ("da", "Dansk"),
    ("de", "Deutsch"),
    ("el", "Ελληνικά"),
    ("en", "English"),
    ("es", "Español"),
    ("et", "Eesti"),
    ("eu", "Euskara"),
    ("fa", "فارسی"),
    ("fi", "Suomi"),
    ("fr", "Français"),
    ("ga", "Gaeilge"),
    ("gl", "Galego"),
    ("he", "עברית"),
    ("hi", "हिन्दी"),
    ("hr", "Hrvatski"),
    ("hu", "Magyar"),
    ("hy", "Հայերեն"),
    ("id", "Bahasa Indonesia"),
    ("is", "Íslenska"),
    ("it", "Italiano"),
    ("ka", "ქართული"),
    ("kk", "Қазақша"),
    ("ko", "한국어"),
    ("lt", "Lietuvių"),
    ("lv", "Latviešu"),
    ("mk", "Македонски"),
    ("nb", "Norsk bokmål"),
    ("nl", "Nederlands"),
    ("nn", "Norsk nynorsk"),
    ("no", "Norsk"),
    ("pl", "Polski"),
    ("pt", "Português"),
    ("ro", "Română"),
    ("ru", "Русский"),
    ("sk", "Slovenčina"),
    ("sl", "Slovenščina"),
    ("sq", "Shqip"),
    ("sr", "Српски"),
    ("sv", "Svenska"),
    ("ta", "தமிழ்"),
    ("th", "ไทย"),
    ("tr", "Türkçe"),
    ("uk", "Українська"),
    ("uz", "Oʻzbekcha"),
    ("vi", "Tiếng Việt"),
];

/// A dictionary code as a human reads it: the language named in itself
/// ("Русский (Россия)" for ru_RU). Unknown regions fall back to the language
/// name plus the raw region; unknown languages to the raw code — never
/// nothing.
pub(crate) fn language_display_name(code: &str) -> String {
    if let Some((_, name)) = LANGUAGE_NAMES.iter().find(|(c, _)| *c == code) {
        return (*name).to_string();
    }
    let lang = code.split(['_', '-']).next().unwrap_or(code);
    if let Some((_, name)) = LANGUAGE_FAMILY_NAMES.iter().find(|(c, _)| *c == lang) {
        return match code.split(['_', '-']).nth(1) {
            Some(region) => format!("{name} ({region})"),
            None => (*name).to_string(),
        };
    }
    code.to_string()
}

/// Subject-line prefixes and mail shorthand no dictionary carries.
const MAIL_WORDS: &[&str] = &["Re", "RE", "re", "Fwd", "FWD", "Fw", "FW"];

/// The words in `text` worth checking, as (byte start, byte end, word).
/// Whitespace-separated tokens carrying an '@', a digit, or a scheme are
/// skipped whole — addresses, versions and links aren't prose — and within
/// the rest, maximal alphabetic runs (apostrophes included) of two letters
/// or more are the words.
fn checkable_words(text: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut token_start = 0;
    for token in text.split_whitespace() {
        let start = token_start + text[token_start..].find(token).unwrap_or(0);
        token_start = start + token.len();
        if token.contains('@') || token.contains("://") || token.chars().any(|c| c.is_ascii_digit())
        {
            continue;
        }
        let mut word_start: Option<usize> = None;
        // One past the end so a trailing word closes like an interior one.
        for (i, ch) in token
            .char_indices()
            .chain(std::iter::once((token.len(), ' ')))
        {
            let wordish = ch.is_alphabetic() || ch == '\'' || ch == '\u{2019}';
            match (word_start, wordish) {
                (None, true) => word_start = Some(i),
                (Some(ws), false) => {
                    let w = &token[ws..i];
                    if w.chars().filter(|c| c.is_alphabetic()).count() >= 2
                        && !MAIL_WORDS.contains(&w)
                    {
                        out.push((start + ws, start + i, w));
                    }
                    word_start = None;
                }
                _ => {}
            }
        }
    }
    out
}

/// Whether one word is misspelled under the current preference and
/// languages. `false` whenever checking is off or unavailable.
pub(crate) fn word_is_misspelled(word: &str) -> bool {
    if !crate::config::load_privacy().spellcheck {
        return false;
    }
    let checkers = checkers_for(&resolved_spell_languages());
    misspelled_in_all(&checkers, word, |c, w| c.is_misspelled(w))
}

/// Pango error-underline attributes for every misspelled word in `text` —
/// `None` (clear the entry's attributes) when checking is off or enchant is
/// unavailable. A word whose byte range contains `cursor` is left unmarked:
/// while the cursor sits in a word it is still being typed, and flagging a
/// half-word on every keystroke reads as nagging. Pass `None` (after a
/// typing pause) to check the cursor's word too.
pub(crate) fn error_attrs(text: &str, cursor: Option<usize>) -> Option<gtk::pango::AttrList> {
    if !crate::config::load_privacy().spellcheck {
        return None;
    }
    let checkers = checkers_for(&resolved_spell_languages());
    if checkers.is_empty() {
        return None;
    }
    let attrs = gtk::pango::AttrList::new();
    for (start, end, word) in checkable_words(text) {
        if cursor.is_some_and(|c| c >= start && c <= end) {
            continue;
        }
        if misspelled_in_all(&checkers, word, |c, w| c.is_misspelled(w)) {
            let mut a = gtk::pango::AttrInt::new_underline(gtk::pango::Underline::Error);
            a.set_start_index(start as u32);
            a.set_end_index(end as u32);
            attrs.insert(a);
            // The error underline takes the text color unless told
            // otherwise; misspellings are red (GNOME's @error_color).
            let mut c = gtk::pango::AttrColor::new_underline_color(0xe0e0, 0x1b1b, 0x2424);
            c.set_start_index(start as u32);
            c.set_end_index(end as u32);
            attrs.insert(c);
        }
    }
    Some(attrs)
}

/// Where enchant keeps the personal word lists, one `<lang>.dic` each.
fn enchant_dir() -> std::path::PathBuf {
    gtk::glib::user_config_dir().join("enchant")
}

/// The words of a personal word list: one a line, trimmed, blank lines
/// skipped.
fn list_words(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(str::trim).filter(|l| !l.is_empty())
}

/// The personal word list of each active language: the plain
/// one-word-per-line file enchant keeps in the user config dir, the same
/// list the body's "Learn Spelling" menu item feeds.
fn personal_dict_paths() -> Vec<std::path::PathBuf> {
    let dir = enchant_dir();
    resolved_spell_languages()
        .iter()
        .map(|lang| dir.join(format!("{lang}.dic")))
        .collect()
}

/// The words the user has taught the spell checker, across the active
/// languages, each once.
pub(crate) fn personal_words() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for path in personal_dict_paths() {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        for word in list_words(&text) {
            if !out.iter().any(|w| w == word) {
                out.push(word.to_string());
            }
        }
    }
    out
}

/// Every language's personal word list in the enchant config dir, for a
/// settings backup: language → words.
pub(crate) fn all_personal_words() -> std::collections::BTreeMap<String, Vec<String>> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(enchant_dir()) else {
        return out;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("dic") {
            continue;
        }
        let Some(lang) = path.file_stem().and_then(|x| x.to_str()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let words: Vec<String> = list_words(&text).map(String::from).collect();
        if !words.is_empty() {
            out.insert(lang.to_string(), words);
        }
    }
    out
}

/// Add a backup's words for `lang` to its personal list, keeping what is
/// already there (nothing is removed). The file is enchant's own
/// one-word-per-line list, created when absent.
pub(crate) fn merge_personal_words(lang: &str, words: &[String]) {
    if words.is_empty() || lang.is_empty() || lang.contains('/') || lang.contains("..") {
        return;
    }
    let dir = enchant_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{lang}.dic"));
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut have: std::collections::BTreeSet<String> =
        list_words(&existing).map(String::from).collect();
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for w in words.iter().map(|w| w.trim()).filter(|w| !w.is_empty()) {
        if have.insert(w.to_string()) {
            out.push_str(w);
            out.push('\n');
        }
    }
    let _ = std::fs::write(&path, out);
    reset_checkers();
}

/// Teach the checker a word: through enchant itself, so the file is written
/// the way its own tooling writes it, and the subject's checker accepts the
/// word immediately. The message body's checker (WebKit's own enchant
/// instance) picks it up at the next launch. With several languages the
/// word joins the first one's list, which is enough: a word any one of the
/// dictionaries accepts is not underlined.
pub(crate) fn add_personal_word(word: &str) {
    let word = word.trim();
    if word.is_empty() {
        return;
    }
    let langs = resolved_spell_languages();
    if let Some(c) = checkers_for(&langs[..langs.len().min(1)]).first() {
        c.learn(word);
    }
}

/// Unlearn a word: enchant's own "remove" would blacklist it instead of
/// forgetting it, so each active language's personal list is rewritten
/// without the word (the body's Learn Spelling may have put it in any of
/// them) and the cached checkers are dropped to reload the trimmed lists.
/// The message body applies the change at the next launch.
pub(crate) fn remove_personal_word(word: &str) {
    for path in personal_dict_paths() {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let kept: Vec<&str> = content
            .lines()
            .filter(|l| l.trim() != word && !l.trim().is_empty())
            .collect();
        let mut out = kept.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        let _ = std::fs::write(&path, out);
    }
    reset_checkers();
}

#[cfg(test)]
mod tests {
    use super::{
        checkable_words, join_languages, language_display_name, misspelled_in_all, parse_languages,
        resolve_languages, toggle_language,
    };

    #[test]
    fn words_are_found_and_noise_is_skipped() {
        let words: Vec<&str> =
            checkable_words("Re: meet ada@x.com at 3pm — v1.2 recieve https://x.y ok")
                .iter()
                .map(|(_, _, w)| *w)
                .collect();
        // "Re" is mail shorthand, the address/version/time/link tokens carry
        // digits or schemes, and single letters aren't words. "at" IS a word
        // — short, but the dictionary knows it, so it never underlines.
        assert_eq!(words, ["meet", "at", "recieve", "ok"]);
    }

    #[test]
    fn byte_ranges_point_at_the_words() {
        let text = "héllo wörld";
        for (s, e, w) in checkable_words(text) {
            assert_eq!(&text[s..e], w);
        }
        assert_eq!(checkable_words(text).len(), 2);
    }

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_listed_language_resolves_once() {
        let dicts = codes(&["de_DE", "en_GB", "en_US", "fr_FR"]);
        assert_eq!(
            resolve_languages("en_US, de_DE;fr_FR", None, &dicts),
            ["en_US", "de_DE", "fr_FR"]
        );
        // Written twice, or twice through a same-language stand-in: once.
        assert_eq!(resolve_languages("de_DE, de_DE", None, &dicts), ["de_DE"]);
        assert_eq!(resolve_languages("de_AT, de_DE", None, &dicts), ["de_DE"]);
    }

    #[test]
    fn a_missing_dictionary_falls_back_or_drops_out() {
        let dicts = codes(&["de_DE", "en_GB"]);
        // Same language, other region.
        assert_eq!(
            resolve_languages("en_US, de_DE", None, &dicts),
            ["en_GB", "de_DE"]
        );
        // No dictionary in that language: left out while another is there.
        assert_eq!(resolve_languages("fi_FI, de_DE", None, &dicts), ["de_DE"]);
        // Nothing usable at all: any dictionary beats checking nothing.
        assert_eq!(resolve_languages("fi_FI", None, &dicts), ["de_DE"]);
        // No dictionaries visible: the codes go through as written.
        assert_eq!(
            resolve_languages("fi_FI, sv_SE", None, &[]),
            ["fi_FI", "sv_SE"]
        );
    }

    #[test]
    fn an_empty_setting_follows_the_locale() {
        let dicts = codes(&["de_DE", "en_US"]);
        assert_eq!(
            resolve_languages("", Some("de_AT".into()), &dicts),
            ["de_DE"]
        );
        assert_eq!(resolve_languages(" , ", None, &dicts), ["en_US"]);
    }

    #[test]
    fn toggling_moves_between_the_system_language_and_a_list() {
        let system = codes(&["en_US"]);
        assert_eq!(toggle_language("", "de_DE", true, &system), "de_DE");
        assert_eq!(
            toggle_language("de_DE", "fr_FR", true, &system),
            "de_DE, fr_FR"
        );
        assert_eq!(
            toggle_language("de_DE, fr_FR", "fr_FR", true, &system),
            "de_DE, fr_FR"
        );
        assert_eq!(
            toggle_language("de_DE, fr_FR", "de_DE", false, &system),
            "fr_FR"
        );
        // The last one off is the system language again.
        assert_eq!(toggle_language("fr_FR", "fr_FR", false, &system), "");
        // The system language on clears the list; off keeps what it checked.
        assert_eq!(toggle_language("de_DE, fr_FR", "", true, &system), "");
        assert_eq!(toggle_language("", "", false, &system), "en_US");
        assert_eq!(toggle_language("de_DE", "", false, &system), "de_DE");
    }

    #[test]
    fn a_word_is_correct_when_any_dictionary_knows_it() {
        let en = ["hello", "world"];
        let de = ["hallo", "welt"];
        let both: [&[&str]; 2] = [&en, &de];
        let rejects = |dict: &&[&str], w: &str| !dict.contains(&w);
        assert!(!misspelled_in_all(&both, "hello", rejects));
        assert!(!misspelled_in_all(&both, "welt", rejects));
        assert!(misspelled_in_all(&both, "wrold", rejects));
        // No dictionary at all flags nothing.
        let none: [&[&str]; 0] = [];
        assert!(!misspelled_in_all(&none, "wrold", rejects));
    }

    // Characterization tests (restructure, area D).

    #[test]
    fn pinned_settings_parse_and_join() {
        assert_eq!(
            parse_languages(" en_US;de_DE  fr,en_US,, "),
            codes(&["en_US", "de_DE", "fr"])
        );
        assert_eq!(join_languages(&codes(&["en_US", "de_DE"])), "en_US, de_DE");
        assert_eq!(parse_languages(""), Vec::<String>::new());
    }

    #[test]
    fn pinned_display_names_fall_back_to_the_code() {
        assert_eq!(language_display_name("xx_YY"), "xx_YY");
        assert_eq!(language_display_name("de"), "Deutsch");
        assert_eq!(language_display_name("de_DE"), "Deutsch (Deutschland)");
        assert_eq!(language_display_name("de_ZZ"), "Deutsch (ZZ)");
    }
}
