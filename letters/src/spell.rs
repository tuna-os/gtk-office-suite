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
            buf.connect_changed(move |_| {
                let sc2 = sc.clone();
                glib::source::idle_add_local_once(move || {
                    if let Ok(s) = sc2.try_borrow() { s.check_all(); }
                });
            })
        };
        Some(SpellCheckHandle { checker: sc, _handler: Rc::new(handler) })
    }

    pub fn add_word(&self, word: &str) {
        self.user_words.borrow_mut().insert(word.to_lowercase());
        self.check_all();
    }

    pub fn suggestions(&self, word: &str) -> Vec<String> {
        let dict = self.dict.borrow();
        match dict.as_ref() {
            Some(d) => basic_suggest(d, word),
            None => vec![],
        }
    }

    fn is_correct(&self, word: &str) -> bool {
        let w = word.to_lowercase();
        if w.len() < 2 || w.chars().all(|c| c.is_ascii_digit()) { return true; }
        if self.user_words.borrow().contains(&w) { return true; }
        self.dict.borrow().as_ref().map(|d| d.check(&w)).unwrap_or(true)
    }

    fn load_dictionary() -> Option<zspell::Dictionary> {
        let candidates = &[
            "/usr/share/hunspell/en_US",
            "/usr/share/myspell/en_US",
            "/usr/local/share/hunspell/en_US",
            "/app/share/hunspell/en_US",
        ];
        for prefix in candidates {
            let dic_path = format!("{}.dic", prefix);
            if std::path::Path::new(&dic_path).exists() {
                match Self::load_dict(prefix) {
                    Ok(d) => { eprintln!("SpellChecker: loaded {}", prefix); return Some(d); }
                    Err(e) => { eprintln!("SpellChecker: failed {}: {:?}", prefix, e); }
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

    fn check_all(&self) {
        let start = self.buffer.start_iter();
        let end = self.buffer.end_iter();
        self.buffer.remove_tag(&self.error_tag, &start, &end);
        let text = self.buffer.text(&start, &end, false).to_string();
        let mut pos = 0usize;
        let bytes = text.as_bytes();
        while pos < bytes.len() {
            while pos < bytes.len() && !is_word_byte(bytes[pos]) { pos += 1; }
            if pos >= bytes.len() { break; }
            let ws = pos;
            while pos < bytes.len() && is_word_byte(bytes[pos]) { pos += 1; }
            let word = &text[ws..pos];
            if !self.is_correct(word) {
                let mut si = start.clone(); si.forward_chars(ws as i32);
                let mut ei = start.clone(); ei.forward_chars(pos as i32);
                self.buffer.apply_tag(&self.error_tag, &si, &ei);
            }
        }
    }
}

#[derive(Clone)]
pub struct SpellCheckHandle {
    checker: Rc<RefCell<SpellChecker>>,
    _handler: Rc<glib::SignalHandlerId>,
}

impl SpellCheckHandle {
    pub fn word_at_cursor(&self, buffer: &gtk::TextBuffer) -> Option<String> {
        let ins = buffer.cursor_position();
        let mut start = buffer.iter_at_offset(ins);
        let mut end = start.clone();
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

    /// Replace the last-found word with a new word.
    pub fn replace_last(&self, buffer: &gtk::TextBuffer, replacement: &str) {
        let ins = buffer.cursor_position();
        let mut start = buffer.iter_at_offset(ins);
        let mut end = start.clone();
        while start.backward_char() {
            let c = start.char();
            if !c.is_ascii_alphabetic() && c != '\'' { start.forward_char(); break; }
        }
        while end.forward_char() {
            let c = end.char();
            if !c.is_ascii_alphabetic() && c != '\'' { break; }
        }
        if start.offset() < end.offset() {
            buffer.begin_user_action();
            buffer.delete(&mut start, &mut end);
            buffer.insert(&mut start, replacement);
            buffer.end_user_action();
        }
    }

    pub fn set_current_word(&self, word: &str) {
        if let Ok(mut s) = self.checker.try_borrow_mut() {
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
    // Transpositions
    for i in 0..n-1 {
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

fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'\''
}

// ── Spell popup menu builder ────────────────────────────────────────────

// ── Spell popup menu builder ────────────────────────────────────────────

/// Find the word at a given (x, y) position in the text view.
pub fn word_at_point(tv: &gtk::TextView, x: f64, y: f64) -> Option<String> {
    // Convert widget coords to buffer position using text view's coordinate mapping
    let (bx, by) = tv.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
    if let Some(iter) = tv.iter_at_location(bx, by) {
        let buf = tv.buffer();
        if iter.offset() < 0 { return None; }
        let mut start = iter.clone();
        let mut end = iter;
        // Expand backward to word start
        while start.backward_char() {
            let c = start.char();
            if !c.is_ascii_alphabetic() && c != '\'' { start.forward_char(); break; }
        }
        // Expand forward to word end
        while end.forward_char() {
            let c = end.char();
            if !c.is_ascii_alphabetic() && c != '\'' { break; }
        }
        let word = buf.text(&start, &end, false).to_string();
        if word.len() >= 2 && word.chars().any(|c| c.is_ascii_alphabetic()) {
            return Some(word);
        }
    }
    None
}

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
