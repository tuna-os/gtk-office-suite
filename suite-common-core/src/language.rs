// language.rs — the contract for the suite's shared language services
// (INTEGRATION-PLAN A9, ADR 0014).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Spelling, grammar and translation are done by different engines in different
// apps: hunspell in Letters, Harper and LanguageTool in Lifafa. ADR 0014 keeps
// them as libraries rather than a service, and the licence (Lifafa's code is
// AGPL) means the sharing goes by contract, not by moving code: this module is
// the part every adapter agrees on, with no engine in it.
//
// * [`LanguageTag`]: the language a piece of text is in, from a locale
//   (`en_US.UTF-8`) or a tag (`en-us`), in one canonical spelling.
// * [`Finding`]: one problem an engine found, as a place in the text.
// * [`Checker`]: what an engine implements.
// * [`merge`]: what several engines found for the same text, with overlaps
//   resolved the same way in every app.

use std::ops::Range;

/// A language, as a BCP 47 tag: `en`, `en-US`, `pt-BR`, `zh-Hant`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct LanguageTag(String);

impl LanguageTag {
    /// Read a locale name or a language tag. `en_US.UTF-8`, `en_US@euro`,
    /// `EN-us` and `en` all work; `C`, `POSIX` and anything that is not a
    /// language give `None`.
    pub fn parse(input: &str) -> Option<Self> {
        // Drop the codeset (`.UTF-8`) and modifier (`@euro`) of a POSIX locale.
        let base = input.trim().split(['.', '@']).next().unwrap_or("");
        let mut parts = base.split(['_', '-']).filter(|p| !p.is_empty());
        let language = parts.next()?;
        if language.eq_ignore_ascii_case("c") || language.eq_ignore_ascii_case("posix") {
            return None;
        }
        if !(2..=3).contains(&language.len()) || !language.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }

        let mut tag = language.to_ascii_lowercase();
        for part in parts {
            if !part.chars().all(|c| c.is_ascii_alphanumeric()) {
                return None;
            }
            tag.push('-');
            match part.len() {
                // A script: `Hant`.
                4 if part.chars().all(|c| c.is_ascii_alphabetic()) => {
                    let mut chars = part.chars();
                    tag.extend(chars.next().map(|c| c.to_ascii_uppercase()));
                    tag.push_str(&chars.as_str().to_ascii_lowercase());
                }
                // A region: `US`, `419`.
                2 | 3 => tag.push_str(&part.to_ascii_uppercase()),
                _ => tag.push_str(&part.to_ascii_lowercase()),
            }
        }
        Some(Self(tag))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The language without its region or script: `en` for `en-US`.
    pub fn primary(&self) -> &str {
        self.0.split('-').next().unwrap_or(&self.0)
    }

    /// Whether text in `self` can be checked by an engine that knows `other`:
    /// the same tag, or the same primary language when the engine has no
    /// regional variant of its own.
    pub fn is_served_by(&self, other: &LanguageTag) -> bool {
        self == other || (!other.0.contains('-') && self.primary() == other.primary())
    }
}

/// What kind of problem a finding is. The order is the precedence when two
/// findings cover the same text: a misspelling hides a grammar finding on the
/// same word, which hides a style one.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FindingKind {
    Spelling,
    Grammar,
    Style,
}

/// One problem found in a text: where, what, and what to put instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    /// Byte range in the text that was checked.
    pub range: Range<usize>,
    pub kind: FindingKind,
    /// What is wrong, in words for the person.
    pub message: String,
    /// Replacements for the range, best first.
    pub suggestions: Vec<String>,
}

impl Finding {
    fn overlaps(&self, other: &Finding) -> bool {
        self.range.start < other.range.end && other.range.start < self.range.end
    }
}

/// What a language engine implements. An engine may be slow (a process, a
/// network call), so callers run `check` off the main thread.
pub trait Checker: Send + Sync {
    /// A name for settings and diagnostics: `hunspell`, `harper`, `languagetool`.
    fn name(&self) -> &str;

    /// The languages this engine can check.
    fn languages(&self) -> Vec<LanguageTag>;

    /// The findings in `text`, which is in `language`. Byte ranges refer to
    /// `text`. An engine that cannot check `language` returns nothing.
    fn check(&self, text: &str, language: &LanguageTag) -> Vec<Finding>;
}

/// Combine what several engines found in one text.
///
/// Findings are sorted by position. Where two overlap, the one of the more
/// important kind is kept (spelling, then grammar, then style), and of two of
/// the same kind the one that starts first, then the shorter. Findings with an
/// empty range are dropped.
pub fn merge(findings: impl IntoIterator<Item = Finding>) -> Vec<Finding> {
    let mut all: Vec<Finding> = findings.into_iter().filter(|f| f.range.start < f.range.end).collect();
    // Most important first, so a kept finding is never displaced by a lesser one.
    all.sort_by(|a, b| {
        a.kind.cmp(&b.kind).then(a.range.start.cmp(&b.range.start)).then((a.range.end - a.range.start).cmp(&(b.range.end - b.range.start)))
    });

    let mut kept: Vec<Finding> = Vec::new();
    for finding in all {
        if !kept.iter().any(|k| k.overlaps(&finding)) {
            kept.push(finding);
        }
    }
    kept.sort_by_key(|f| (f.range.start, f.range.end));
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &str) -> LanguageTag {
        LanguageTag::parse(s).unwrap_or_else(|| panic!("{s} should parse"))
    }

    fn finding(range: Range<usize>, kind: FindingKind) -> Finding {
        Finding { range, kind, message: String::new(), suggestions: Vec::new() }
    }

    #[test]
    fn locales_and_tags_reach_one_spelling() {
        for input in ["en_US.UTF-8", "en_US@euro", "EN-us", "en-US", " en_us "] {
            assert_eq!(tag(input).as_str(), "en-US", "{input}");
        }
        assert_eq!(tag("de").as_str(), "de");
        assert_eq!(tag("pt_BR").as_str(), "pt-BR");
        assert_eq!(tag("zh-hant-tw").as_str(), "zh-Hant-TW");
        assert_eq!(tag("es_419").as_str(), "es-419");
    }

    #[test]
    fn what_is_not_a_language_is_refused() {
        for input in ["", "C", "POSIX", "C.UTF-8", "e", "english", "12", "en_%%"] {
            assert!(LanguageTag::parse(input).is_none(), "{input}");
        }
    }

    #[test]
    fn the_primary_language_drops_region_and_script() {
        assert_eq!(tag("en-US").primary(), "en");
        assert_eq!(tag("zh-Hant-TW").primary(), "zh");
        assert_eq!(tag("fr").primary(), "fr");
    }

    #[test]
    fn an_engine_without_a_variant_serves_every_region_of_its_language() {
        assert!(tag("en-GB").is_served_by(&tag("en")));
        assert!(tag("en-GB").is_served_by(&tag("en-GB")));
        // An engine that has its own regional dictionary is not stretched to another region.
        assert!(!tag("en-GB").is_served_by(&tag("en-US")));
        assert!(!tag("fr").is_served_by(&tag("en")));
    }

    #[test]
    fn a_misspelling_hides_a_grammar_finding_on_the_same_word() {
        let merged = merge([finding(4..9, FindingKind::Grammar), finding(4..9, FindingKind::Spelling)]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].kind, FindingKind::Spelling);
    }

    #[test]
    fn overlap_is_about_the_range_not_just_equal_ranges() {
        let merged = merge([finding(0..10, FindingKind::Style), finding(8..12, FindingKind::Grammar)]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].kind, FindingKind::Grammar);
    }

    #[test]
    fn findings_that_touch_but_do_not_overlap_are_both_kept_in_order() {
        let merged = merge([finding(5..9, FindingKind::Spelling), finding(0..5, FindingKind::Grammar)]);
        assert_eq!(merged.iter().map(|f| f.range.clone()).collect::<Vec<_>>(), [0..5, 5..9]);
    }

    #[test]
    fn of_two_of_one_kind_the_earlier_start_wins_then_the_shorter() {
        let merged = merge([finding(3..8, FindingKind::Grammar), finding(1..5, FindingKind::Grammar)]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].range, 1..5);
        let merged = merge([finding(2..9, FindingKind::Style), finding(2..4, FindingKind::Style)]);
        assert_eq!(merged[0].range, 2..4);
    }

    #[test]
    fn empty_ranges_are_dropped() {
        assert!(merge([finding(3..3, FindingKind::Spelling)]).is_empty());
    }

    #[test]
    fn a_checker_is_usable_as_a_trait_object() {
        struct Shouty;
        impl Checker for Shouty {
            fn name(&self) -> &str {
                "shouty"
            }
            fn languages(&self) -> Vec<LanguageTag> {
                vec![LanguageTag::parse("en").unwrap()]
            }
            fn check(&self, text: &str, language: &LanguageTag) -> Vec<Finding> {
                if !language.is_served_by(&self.languages()[0]) {
                    return Vec::new();
                }
                text.match_indices("TEH").map(|(i, w)| Finding {
                    range: i..i + w.len(),
                    kind: FindingKind::Spelling,
                    message: "not a word".into(),
                    suggestions: vec!["the".into()],
                }).collect()
            }
        }
        let engines: Vec<Box<dyn Checker>> = vec![Box::new(Shouty)];
        let found = merge(engines.iter().flat_map(|e| e.check("TEH cat", &tag("en-GB"))));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].suggestions, ["the"]);
        assert!(engines[0].check("TEH cat", &tag("fr")).is_empty());
    }
}
