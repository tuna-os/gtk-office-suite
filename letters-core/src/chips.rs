// SPDX-License-Identifier: GPL-3.0-or-later
//
// chips.rs — smart chips (DESIGN-UI, Docs: "smart chips (dates, people,
// links) as inline objects").
//
// A chip is an inline object like an image: one char of the edit sequence
// and of the layout text, so a caret steps over it and Delete removes it
// whole. The run's text is the chip's label, what a reader sees; the
// `Chip` on its style says what the label stands for: a date (ISO), a
// person (an email address, or just a name) or a link (a URL).
//
// Link and person chips also carry `RunStyle::link` (the URL, or mailto:),
// so every writer saves a chip as its label, and as a hyperlink where it
// has one, without knowing about chips.

use crate::model::{Run, RunStyle};
pub use chrono::{Datelike, NaiveDate};
use chrono::Duration;

/// Today, in local time: what "Today" means in the "@" menu.
pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChipKind {
    Date,
    Person,
    Link,
}

/// What a chip stands for. The label is the run's text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chip {
    pub kind: ChipKind,
    /// A date as `YYYY-MM-DD`, a person's email address (or their name
    /// when there is none), or a link's URL.
    pub value: String,
}

/// A date's label: "25 Sep 2026".
pub fn date_label(date: NaiveDate) -> String {
    date.format("%-d %b %Y").to_string()
}

/// The run for `chip` labelled `label`.
pub fn chip_run(chip: Chip, label: impl Into<String>) -> Run {
    let link = match chip.kind {
        ChipKind::Link => Some(chip.value.clone()),
        ChipKind::Person if chip.value.contains('@') => Some(format!("mailto:{}", chip.value)),
        _ => None,
    };
    Run { text: label.into(), style: RunStyle { chip: Some(chip), link, ..Default::default() } }
}

/// A date chip for `date`.
pub fn date_chip(date: NaiveDate) -> Run {
    chip_run(Chip { kind: ChipKind::Date, value: date.format("%Y-%m-%d").to_string() }, date_label(date))
}

/// Parse a date as people type one: an ISO date, "25 Sep 2026",
/// "Sep 25 2026", "25/09/2026" (day first), or "25 Sep" (this year).
fn parse_date(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    let s = input.trim().trim_end_matches('.').replace(',', "");
    for f in ["%Y-%m-%d", "%d %b %Y", "%d %B %Y", "%b %d %Y", "%B %d %Y", "%d/%m/%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(&s, f) {
            return Some(d);
        }
    }
    for f in ["%d %b %Y", "%d %B %Y", "%b %d %Y", "%B %d %Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(&format!("{s} {}", today.year()), f) {
            return Some(d);
        }
    }
    None
}

/// The relative dates offered by name.
const RELATIVE: [(&str, i64); 3] = [("Today", 0), ("Tomorrow", 1), ("Yesterday", -1)];

fn is_url(s: &str) -> bool {
    (s.starts_with("https://") || s.starts_with("http://") || s.starts_with("www."))
        && !s.contains(char::is_whitespace)
        && s.len() > 8
}

/// A link's label: the URL without its scheme or trailing slash.
fn link_label(url: &str) -> String {
    let s = url.trim_start_matches("https://").trim_start_matches("http://");
    s.trim_end_matches('/').to_string()
}

/// "Ada Lovelace <ada@example.org>", "ada@example.org" or "Ada Lovelace".
fn parse_person(s: &str) -> Option<Run> {
    if let (Some(open), Some(close)) = (s.find('<'), s.rfind('>')) {
        let name = s[..open].trim();
        let email = s[open + 1..close].trim();
        if email.contains('@') && !name.is_empty() {
            return Some(chip_run(Chip { kind: ChipKind::Person, value: email.to_string() }, name));
        }
    }
    if s.contains('@') && !s.contains(char::is_whitespace) {
        let local = s.split('@').next().unwrap_or(s);
        return Some(chip_run(Chip { kind: ChipKind::Person, value: s.to_string() }, local));
    }
    let name = s.trim();
    let plausible = !name.is_empty()
        && name.split_whitespace().count() <= 4
        && name.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '\'' || c == '.');
    plausible.then(|| chip_run(Chip { kind: ChipKind::Person, value: name.to_string() }, name))
}

/// The chips an "@" query offers, best first: relative dates whose name
/// starts with the query (all three when it is empty), a date it spells, a
/// link it is, or the person it names.
pub fn suggestions(query: &str, today: NaiveDate) -> Vec<Run> {
    let q = query.trim();
    let mut out: Vec<Run> = RELATIVE
        .iter()
        .filter(|(name, _)| name.to_lowercase().starts_with(&q.to_lowercase()))
        .map(|(_, days)| date_chip(today + Duration::days(*days)))
        .collect();
    if q.is_empty() {
        return out;
    }
    if let Some(d) = parse_date(q, today) {
        out.push(date_chip(d));
    } else if is_url(q) {
        let url = if q.starts_with("www.") { format!("https://{q}") } else { q.to_string() };
        out.push(chip_run(Chip { kind: ChipKind::Link, value: url.clone() }, link_label(&url)));
    } else if out.is_empty() {
        out.extend(parse_person(q));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn describe(runs: &[Run]) -> Vec<(ChipKind, String, String)> {
        runs.iter()
            .map(|r| {
                let c = r.style.chip.clone().unwrap();
                (c.kind, c.value, r.text.clone())
            })
            .collect()
    }

    #[test]
    fn an_empty_query_offers_the_relative_dates() {
        let got = describe(&suggestions("", day(2026, 9, 25)));
        assert_eq!(
            got,
            [
                (ChipKind::Date, "2026-09-25".into(), "25 Sep 2026".into()),
                (ChipKind::Date, "2026-09-26".into(), "26 Sep 2026".into()),
                (ChipKind::Date, "2026-09-24".into(), "24 Sep 2026".into()),
            ]
        );
        assert_eq!(describe(&suggestions("tom", day(2026, 12, 31)))[0].1, "2027-01-01");
    }

    #[test]
    fn dates_are_read_as_people_type_them() {
        let today = day(2026, 9, 25);
        for q in ["2026-10-03", "3 Oct 2026", "3 October 2026", "Oct 3, 2026", "03/10/2026", "3 Oct"] {
            assert_eq!(describe(&suggestions(q, today)), [(ChipKind::Date, "2026-10-03".into(), "3 Oct 2026".into())], "{q}");
        }
    }

    #[test]
    fn links_and_people_carry_their_hyperlink() {
        let today = day(2026, 9, 25);
        let link = &suggestions("https://gnome.org/", today)[0];
        assert_eq!((link.text.as_str(), link.style.link.as_deref()), ("gnome.org", Some("https://gnome.org/")));
        let www = &suggestions("www.example.org", today)[0];
        assert_eq!(www.style.chip.as_ref().unwrap().value, "https://www.example.org");
        let ada = &suggestions("Ada Lovelace <ada@example.org>", today)[0];
        assert_eq!((ada.text.as_str(), ada.style.link.as_deref()), ("Ada Lovelace", Some("mailto:ada@example.org")));
        let bare = &suggestions("grace@example.org", today)[0];
        assert_eq!(bare.text, "grace");
        let name = &suggestions("Grace Hopper", today)[0];
        assert_eq!((name.style.chip.as_ref().unwrap().kind, name.style.link.as_ref()), (ChipKind::Person, None));
        assert!(suggestions("3 + 4 = 7", today).is_empty(), "not everything is a chip");
    }
}
