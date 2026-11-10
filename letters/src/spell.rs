// SPDX-License-Identifier: GPL-3.0-or-later
//
// Spell-check using zspell (pure Rust hunspell-compatible) for word checking,
// with basic Levenshtein-based suggestions for misspelled words.

use gtk4::{self as gtk, gio, glib, prelude::*};
use std::collections::HashSet;
use std::cell::RefCell;
use std::rc::Rc;

pub struct SpellChecker {
    dict: RefCell<Option<zspell::Dictionary>>,
    user_words: RefCell<HashSet<String>>,
    error_tag: gtk::TextTag,
    buffer: gtk::TextBuffer,
}

impl SpellChecker {
    pub fn new(buffer: &gtk::TextBuffer) -> Self {
        let dict = Self::load_dictionary();
        let tb = buffer.tag_table();
        let error_tag = if let Some(tag) = tb.lookup("spelling-error") {
            tag
        } else {
            let tag = gtk::TextTag::builder()
                .name("spelling-error")
                .underline(gtk4::pango::Underline::Error)
                .underline_rgba(&gtk4::gdk::RGBA::new(1.0, 0.0, 0.0, 1.0))
                .build();
            tb.add(&tag);
            tag
        };
        SpellChecker { dict: RefCell::new(dict), user_words: RefCell::new(HashSet::new()), error_tag, buffer: buffer.clone() }
    }

    pub fn start(self) -> Option<SpellCheckHandle> {
        let sc = Rc::new(RefCell::new(self));
        let buf = sc.borrow().buffer.clone();
        if sc.borrow().dict.borrow().is_none() {
            return None;
        }
        sc.borrow().check_all();
        let handler = {
            let sc = sc.clone();
            // One pending check, not one per keystroke (#1990).
            let idle = crate::idle::IdleOnce::default();
            buf.connect_changed(move |_| {
                let sc2 = sc.clone();
                idle.schedule(move || {
                    if let Ok(s) = sc2.try_borrow() { s.check_all(); }
                });
            })
        };
        Some(SpellCheckHandle { checker: sc, _handler: Rc::new(handler) })
    }

    // add_word/suggestions and the SpellCheckHandle suggestion-menu API
    // below are the backend for a right-click "suggestions" context menu
    // that isn't wired into the text view's popup yet — see word_at_point
    // and make_spell_menu at the bottom of this file for the other half
    // of the planned feature. Kept and #[allow(dead_code)]'d rather than
    // deleted since the checker/dictionary plumbing is already in place.
    #[allow(dead_code)]
    pub fn add_word(&self, word: &str) {
        self.user_words.borrow_mut().insert(word.to_lowercase());
        self.check_all();
    }

    #[allow(dead_code)]
    pub fn suggestions(&self, word: &str) -> Vec<String> {
        let dict = self.dict.borrow();
        match dict.as_ref() {
            Some(d) => basic_suggest(d, word),
            None => vec![],
        }
    }

    fn is_correct(&self, word: &str) -> bool {
        // Hunspell dictionaries spell "don't" with the ASCII apostrophe.
        let w = word.to_lowercase().replace('\u{2019}', "'");
        if w.len() < 2 || w.chars().all(|c| c.is_ascii_digit()) { return true; }
        if self.user_words.borrow().contains(&w) { return true; }
        self.dict.borrow().as_ref().map(|d| d.check(&w)).unwrap_or(true)
    }

    /// The hunspell dictionary for the user's language (#1991): the
    /// locales GLib reads from LANGUAGE, LC_ALL, LC_MESSAGES and LANG, in
    /// order, each looked up in the usual dictionary folders, then en_US.
    fn load_dictionary() -> Option<zspell::Dictionary> {
        let languages: Vec<String> = glib::language_names()
            .iter()
            .map(|l| l.to_string())
            .collect();
        for prefix in dictionary_prefixes(&dictionary_names(&languages), DICTIONARY_DIRS) {
            match Self::load_dict(&prefix) {
                Ok(d) => {
                    eprintln!("SpellChecker: loaded {}", prefix);
                    return Some(d);
                }
                Err(e) => {
                    eprintln!("SpellChecker: failed {}: {:?}", prefix, e);
                }
            }
        }
        eprintln!("SpellChecker: no dictionary found");
        None
    }

    fn load_dict(basepath: &str) -> Result<zspell::Dictionary, Box<dyn std::error::Error>> {
        let aff = std::fs::read_to_string(format!("{}.aff", basepath))?;
        let dic = std::fs::read_to_string(format!("{}.dic", basepath))?;
        let dict = zspell::builder()
            .config_str(&aff)
            .dict_str(&dic)
            .build()?;
        Ok(dict)
    }

    pub(crate) fn check_all(&self) {
        let start = self.buffer.start_iter();
        let end = self.buffer.end_iter();
        self.buffer.remove_tag(&self.error_tag, &start, &end);
        // The words are the model's (LET-003): iteration runs over the
        // live model's sequence text, and only the presentation-only error
        // tag goes onto the buffer. Without a live model there is nothing
        // to check — and no fallback that writes content. Offsets are
        // char counts throughout: the byte indices this used confused
        // non-ASCII words with the chars after them.
        let Some(live) = crate::live::of(&self.buffer) else { return };
        let mut m = live.borrow_mut();
        let chars: Vec<char> = letters_core::edit::sequence_text(m.document()).chars().collect();
        let mut pos = 0;
        while pos < chars.len() {
            while pos < chars.len() && !is_word_char(chars[pos]) { pos += 1; }
            if pos >= chars.len() { break; }
            let ws = pos;
            while pos < chars.len() && is_word_char(chars[pos]) { pos += 1; }
            let word: String = chars[ws..pos].iter().collect();
            if !self.is_correct(&word) {
                if let (Some(s), Some(e)) = (m.buffer_offset(ws), m.buffer_offset(pos)) {
                    self.buffer.apply_tag(
                        &self.error_tag,
                        &self.buffer.iter_at_offset(s as i32),
                        &self.buffer.iter_at_offset(e as i32),
                    );
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct SpellCheckHandle {
    #[allow(dead_code)]
    checker: Rc<RefCell<SpellChecker>>,
    _handler: Rc<glib::SignalHandlerId>,
}

// The methods below back a right-click spelling-suggestions context menu
// that isn't wired into the text view's popup handler yet (see the note
// on SpellChecker::add_word above). Kept in place rather than deleted.
#[allow(dead_code)]
impl SpellCheckHandle {
    pub fn word_at_cursor(&self, buffer: &gtk::TextBuffer) -> Option<String> {
        // The word is the model's (LET-003): the caret's buffer offset maps
        // to a sequence offset, and the word bounds come from the sequence
        // text. Buffer offsets are for caret placement only. Without a live
        // model, fall back to reading the buffer (read-only: no content is
        // written back).
        if let Some(live) = crate::live::of(buffer) {
            let mut m = live.borrow_mut();
            let at = m.sequence_offset(buffer.cursor_position().max(0) as usize);
            let chars: Vec<char> = letters_core::edit::sequence_text(m.document()).chars().collect();
            if at > chars.len() {
                return None;
            }
            let (mut s, mut e) = (at, at);
            while s > 0 && is_word_char(chars[s - 1]) { s -= 1; }
            while e < chars.len() && is_word_char(chars[e]) { e += 1; }
            let word: String = chars[s..e].iter().collect();
            return (word.len() >= 2 && word.chars().any(|c| c.is_ascii_alphabetic())).then_some(word);
        }
        let ins = buffer.cursor_position();
        let mut start = buffer.iter_at_offset(ins);
        let mut end = start;
        while start.backward_char() {
            let c = start.char();
            if !c.is_ascii_alphabetic() && c != '\'' { start.forward_char(); break; }
        }
        while end.forward_char() {
            let c = end.char();
            if !c.is_ascii_alphabetic() && c != '\'' { break; }
        }
        let word = buffer.text(&start, &end, false).to_string();
        if word.len() >= 2 && word.chars().any(|c| c.is_ascii_alphabetic()) { Some(word) } else { None }
    }

    /// Replace the model word at the caret with `replacement`: the backend
    /// a spelling-suggestions menu applies (LET-003). The word's range is a
    /// sequence range from `snapshot().doc`; the buffer offsets derived from
    /// it place the caret only. One model edit, so one Undo takes it back.
    /// False with no live model or when the caret is not on a word.
    #[allow(dead_code)]
    pub fn replace_word_at_caret(&self, buffer: &gtk::TextBuffer, replacement: &str) -> bool {
        let Some(live) = crate::live::of(buffer) else { return false };
        let mut m = live.borrow_mut();
        let at = m.sequence_offset(buffer.cursor_position().max(0) as usize);
        let chars: Vec<char> = letters_core::edit::sequence_text(m.document()).chars().collect();
        if at > chars.len() {
            return false;
        }
        let (mut s, mut e) = (at, at);
        while s > 0 && is_word_char(chars[s - 1]) { s -= 1; }
        while e < chars.len() && is_word_char(chars[e]) { e += 1; }
        let word: String = chars[s..e].iter().collect();
        if word.len() < 2 || !word.chars().any(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let (Some(bs), Some(be)) = (m.buffer_offset(s), m.buffer_offset(e)) else { return false };
        drop(m);
        crate::dialogs::replace_ranges(buffer, &[(bs, be)], replacement)
    }

    pub fn suggestions(&self, word: &str) -> Vec<String> {
        self.checker.try_borrow().map(|s| s.suggestions(word)).unwrap_or_default()
    }

    pub fn add_word(&self, word: &str) {
        if let Ok(s) = self.checker.try_borrow() { s.add_word(word); }
    }

    /// Check if a word is correctly spelled.
    pub fn is_correct(&self, word: &str) -> bool {
        self.checker.try_borrow().map(|s| {
            let w = word.to_lowercase();
            if w.len() < 2 || w.chars().all(|c| c.is_ascii_digit()) { return true; }
            if s.user_words.borrow().contains(&w) { return true; }
            s.dict.borrow().as_ref().map(|d| d.check(&w)).unwrap_or(true)
        }).unwrap_or(true)
    }

    pub fn set_current_word(&self, _word: &str) {
        if let Ok(_s) = self.checker.try_borrow_mut() {
            // No-op placeholder — word is captured in closures
        }
    }
}

// ── Simple suggestion algorithm (Levenshtein + common substitutions) ──

fn basic_suggest(dict: &zspell::Dictionary, word: &str) -> Vec<String> {
    let word_lower = word.to_lowercase();
    let mut results: Vec<(usize, String)> = Vec::new();
    // Generate candidate corrections (1-2 edit distance)
    let candidates = generate_candidates(&word_lower);
    for c in candidates {
        if dict.check(&c) {
            let dist = levenshtein(&word_lower, &c);
            results.push((dist, c));
        }
    }
    results.sort_by_key(|(d, _)| *d);
    results.truncate(8);
    results.into_iter().map(|(_, w)| w).collect()
}

fn generate_candidates(word: &str) -> Vec<String> {
    let mut v = Vec::new();
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    // Deletions
    for i in 0..n {
        let s: String = chars[..i].iter().chain(&chars[i+1..]).collect();
        v.push(s);
    }
    // Transpositions (saturating: an empty word has no adjacent pair to swap)
    for i in 0..n.saturating_sub(1) {
        let mut c = chars.clone();
        c.swap(i, i+1);
        v.push(c.into_iter().collect());
    }
    // Common single-char substitutions
    let subs = [('i','e'),('e','i'),('a','e'),('e','a'),('o','u'),('u','o'),('c','s'),('s','c')];
    for i in 0..n {
        for (a,b) in subs {
            let mut c = chars.clone();
            if c[i] == a { c[i] = b; v.push(c.into_iter().collect()); }
        }
    }
    // Insertions (common double letters)
    for i in 0..=n {
        for ch in ['e','s','l','r','n','t'] {
            let s: String = chars[..i].iter().chain(std::iter::once(&ch)).chain(&chars[i..]).collect();
            v.push(s);
        }
    }
    v
}

fn levenshtein(a: &str, b: &str) -> usize {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let n = ac.len(); let m = bc.len();
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0; m+1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if ac[i-1] == bc[j-1] {0} else {1};
            curr[j] = (prev[j]+1).min(curr[j-1]+1).min(prev[j-1]+cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

/// One word definition for buffer and model iteration alike: any letter,
/// in any script, and the apostrophe, straight or curly. It used to be
/// ASCII letters only, so "café" was checked as "caf" and "naïve" as two
/// fragments (#1991).
fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || c == '\'' || c == '\u{2019}'
}

/// Where distributions and the Flatpak runtime keep hunspell dictionaries.
const DICTIONARY_DIRS: &[&str] = &[
    "/app/share/hunspell",
    "/usr/share/hunspell",
    "/usr/share/myspell",
    "/usr/share/myspell/dicts",
    "/usr/local/share/hunspell",
];

/// Dictionary names to try for `languages` (GLib's list, most preferred
/// first, such as "fr_FR.UTF-8", "fr_FR", "fr", "C"): each locale without
/// its encoding or modifier, then en_US as the last resort. A bare
/// language ("de") stands for any of its regions.
fn dictionary_names(languages: &[String]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for language in languages {
        let name = language.split(['.', '@']).next().unwrap_or("");
        if name.is_empty() || name == "C" || name == "POSIX" {
            continue;
        }
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    if !names.iter().any(|n| n == "en_US") {
        names.push("en_US".to_string());
    }
    names
}

/// The `<dir>/<name>` prefixes, in preference order, whose `.dic` and `.aff`
/// both exist. A bare language name matches its regional dictionaries too
/// ("pt" finds "pt_BR"), after an exact match.
fn dictionary_prefixes(names: &[String], dirs: &[&str]) -> Vec<String> {
    let complete = |prefix: &std::path::Path| {
        prefix.with_extension("dic").is_file() && prefix.with_extension("aff").is_file()
    };
    let mut found: Vec<String> = Vec::new();
    let push = |prefix: std::path::PathBuf, found: &mut Vec<String>| {
        let prefix = prefix.to_string_lossy().into_owned();
        if !found.contains(&prefix) {
            found.push(prefix);
        }
    };
    for name in names {
        for dir in dirs {
            let prefix = std::path::Path::new(dir).join(name);
            if complete(&prefix) {
                push(prefix, &mut found);
            }
        }
        if name.contains('_') {
            continue;
        }
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut regional: Vec<std::path::PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "dic"))
                .filter(|p| {
                    p.file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| s.starts_with(&format!("{name}_")))
                })
                .map(|p| p.with_extension(""))
                .filter(|p| complete(p))
                .collect();
            regional.sort();
            for prefix in regional {
                push(prefix, &mut found);
            }
        }
    }
    found
}



// ── Spell popup menu builder ────────────────────────────────────────────
//
// word_at_point/make_spell_menu are the other half of the right-click
// suggestions menu described above (not wired into the text view's popup
// handler yet). Kept rather than deleted.

#[allow(dead_code)]
pub fn make_spell_menu(word: &str, suggestions: &[String]) -> gio::Menu {
    let menu = gio::Menu::new();
    if !suggestions.is_empty() {
        let sec = gio::Menu::new();
        for s in suggestions.iter().take(5) {
            sec.append(Some(s), Some("spell.apply-suggestion"));
        }
        menu.append_section(None, &sec);
    }
    let act = gio::Menu::new();
    act.append(Some(&format!("Add \"{}\" to Dictionary", word)), Some("spell.add-word"));
    menu.append_section(None, &act);
    menu
}
#[cfg(test)]
mod tests {
    use super::{
        dictionary_names, dictionary_prefixes, generate_candidates, is_word_char, levenshtein,
    };

    #[test]
    fn word_chars_are_letters_in_any_script_and_apostrophes() {
        for c in ['a', 'Z', '\'', '\u{2019}', 'é', 'ï', 'ß', 'Ж', 'ü', '中'] {
            assert!(is_word_char(c), "{c:?}");
        }
        for c in [' ', '0', '-', '.', ',', '"', '!'] {
            assert!(!is_word_char(c), "{c:?}");
        }
    }

    /// "café" and "naïve" are one word each, not "caf" and "na" + "ve".
    #[test]
    fn accented_words_are_not_split() {
        let words: Vec<String> = "café naïve Straße"
            .split(|c: char| !is_word_char(c))
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect();
        assert_eq!(words, ["café", "naïve", "Straße"]);
    }

    #[test]
    fn dictionary_names_follow_the_locale_then_fall_back_to_english() {
        let langs = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            dictionary_names(&langs(&["fr_FR.UTF-8", "fr_FR", "fr", "C"])),
            ["fr_FR", "fr", "en_US"]
        );
        assert_eq!(
            dictionary_names(&langs(&["de_DE@euro", "de"])),
            ["de_DE", "de", "en_US"]
        );
        assert_eq!(dictionary_names(&langs(&["C"])), ["en_US"]);
        assert_eq!(
            dictionary_names(&langs(&["en_US.UTF-8", "en_US", "en"])),
            ["en_US", "en"]
        );
    }

    #[test]
    fn dictionaries_are_found_by_locale_and_by_language() {
        let dir = std::env::temp_dir().join(format!("letters-spell-dicts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["fr_FR", "pt_BR", "en_US"] {
            std::fs::write(dir.join(format!("{name}.dic")), "1\nx\n").unwrap();
            std::fs::write(dir.join(format!("{name}.aff")), "SET UTF-8\n").unwrap();
        }
        // A .dic with no .aff is not a dictionary.
        std::fs::write(dir.join("de_DE.dic"), "1\nx\n").unwrap();
        let d = dir.to_str().unwrap();
        let names = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let at = |n: &str| format!("{d}/{n}");
        assert_eq!(
            dictionary_prefixes(&names(&["fr_FR", "fr", "en_US"]), &[d]),
            [at("fr_FR"), at("en_US")]
        );
        assert_eq!(
            dictionary_prefixes(&names(&["pt", "en_US"]), &[d]),
            [at("pt_BR"), at("en_US")]
        );
        assert_eq!(
            dictionary_prefixes(&names(&["de_DE", "de", "en_US"]), &[d]),
            [at("en_US")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("a", "a"), 0);
        assert_eq!(levenshtein("xyz", ""), 3);
    }

    #[test]
    fn levenshtein_known_edit_distances() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("flaw", "lawn"), 2);
        assert_eq!(levenshtein("abc", "abcd"), 1);
        // Plain Levenshtein counts a transposition as two edits.
        assert_eq!(levenshtein("teh", "the"), 2);
    }

    #[test]
    fn generate_candidates_includes_all_edit_kinds() {
        let c = generate_candidates("cat");
        for want in ["at", "ct", "ca", "act", "cta", "sat", "cet", "ceat", "cats", "scat"] {
            assert!(c.iter().any(|s| s == want), "missing candidate {want}");
        }
    }

    #[test]
    fn generate_candidates_count_matches_edit_span() {
        // n=3 "cat": 3 deletions + 2 transpositions + 2 applicable substitutions
        // ('c'→'s', 'a'→'e') + 4×6 insertions = 31.
        assert_eq!(generate_candidates("cat").len(), 31);
        // n=5 "hello": 5 + 4 + 3 ('e'→'i'/'a', 'o'→'u') + 6×6 = 48.
        assert_eq!(generate_candidates("hello").len(), 48);
    }

    #[test]
    fn generate_candidates_handles_empty_word() {
        // n=0: no deletions, no transpositions (the 0..n-1 underflow of #172),
        // no substitutions — only the 1×6 insertion loop applies.
        let c = generate_candidates("");
        assert_eq!(c.len(), 6);
        for want in ["e", "s", "l", "r", "n", "t"] {
            assert!(c.iter().any(|s| s == want), "missing candidate {want}");
        }
    }

    #[test]
    fn generate_candidates_handles_single_char_word() {
        // n=1 is the other transposition-loop boundary: 1 deletion + 0
        // transpositions + 1 substitution ('a'→'e') + 2×6 insertions = 14.
        assert_eq!(generate_candidates("a").len(), 14);
    }
}
