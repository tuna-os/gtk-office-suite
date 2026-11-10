// people.rs — the GTK-free half of the suite's person picker (INTEGRATION-PLAN A7).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Letters' recipient chips, Decks, Tables and Lifafa's composer all want the
// same thing: type a few letters, see the people (and the address of theirs)
// that match, best first. The contacts themselves come from Evolution Data
// Server in a GTK shell; this module is what that shell hands the people to,
// so the matching and the ordering are one tested rule instead of one per app.

/// A person as the picker sees them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Person {
    pub name: String,
    pub emails: Vec<String>,
}

/// One row of the picker: a person and one of their addresses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Suggestion {
    pub name: String,
    pub email: String,
}

impl Suggestion {
    /// The `Name <address>` form a recipient field holds, or the bare address
    /// when there is no name.
    pub fn display(&self) -> String {
        if self.name.is_empty() {
            self.email.clone()
        } else {
            format!("{} <{}>", self.name, self.email)
        }
    }
}

/// How well one text matches the query, lower is better; `None` for no match.
fn rank(text: &str, query: &str) -> Option<u8> {
    let text = text.to_lowercase();
    if text.starts_with(query) {
        return Some(0);
    }
    // A word inside the text starts with it: "ada" in "Countess Ada Lovelace".
    if text.split(|c: char| !c.is_alphanumeric()).any(|word| !word.is_empty() && word.starts_with(query)) {
        return Some(1);
    }
    text.contains(query).then_some(2)
}

/// The best `limit` suggestions for `query`, best first.
///
/// A person matches by name or by any address; a person with several matching
/// addresses gets one row each, and an address that appears twice (under two
/// people, or twice in the list) is offered once, for the first person who has
/// it. Within a rank the order the people came in is kept, so a caller that
/// sorts its contacts by recent use gets that order back.
pub fn suggest(people: &[Person], query: &str, limit: usize) -> Vec<Suggestion> {
    let query = query.trim().to_lowercase();
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }

    let mut rows: Vec<(u8, usize, Suggestion)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (position, person) in people.iter().enumerate() {
        let by_name = rank(&person.name, &query);
        for email in &person.emails {
            let email = email.trim();
            if email.is_empty() {
                continue;
            }
            let Some(best) = [by_name, rank(email, &query)].into_iter().flatten().min() else {
                continue;
            };
            if seen.insert(email.to_lowercase()) {
                rows.push((best, position, Suggestion { name: person.name.clone(), email: email.to_string() }));
            }
        }
    }
    rows.sort_by_key(|(rank, position, _)| (*rank, *position));
    rows.into_iter().take(limit).map(|(_, _, suggestion)| suggestion).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str, emails: &[&str]) -> Person {
        Person { name: name.into(), emails: emails.iter().map(|e| e.to_string()).collect() }
    }

    fn emails(rows: &[Suggestion]) -> Vec<&str> {
        rows.iter().map(|s| s.email.as_str()).collect()
    }

    #[test]
    fn a_name_prefix_beats_a_word_inside_the_name() {
        let people = [person("Countess Ada Lovelace", &["countess@example.org"]), person("Ada Yonath", &["yonath@example.org"])];
        assert_eq!(emails(&suggest(&people, "ada", 10)), ["yonath@example.org", "countess@example.org"]);
    }

    #[test]
    fn matching_is_case_insensitive_and_trims_the_query() {
        let people = [person("Grace Hopper", &["grace@example.org"])];
        assert_eq!(emails(&suggest(&people, "  GRA ", 10)), ["grace@example.org"]);
    }

    #[test]
    fn an_address_can_match_when_the_name_does_not() {
        let people = [person("G. Hopper", &["navy@example.org"])];
        assert_eq!(emails(&suggest(&people, "navy", 10)), ["navy@example.org"]);
    }

    #[test]
    fn each_matching_address_of_a_person_is_a_row() {
        let people = [person("Ada", &["ada@home.example", "ada@work.example"])];
        assert_eq!(emails(&suggest(&people, "ada", 10)), ["ada@home.example", "ada@work.example"]);
    }

    #[test]
    fn an_address_is_offered_once_for_the_first_person_who_has_it() {
        let people = [person("Ada", &["shared@example.org"]), person("Ada L.", &["Shared@Example.org"])];
        let rows = suggest(&people, "ada", 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Ada");
    }

    #[test]
    fn the_order_people_came_in_is_kept_within_a_rank() {
        let people = [person("Bea", &["b@example.org"]), person("Beatrice", &["bt@example.org"]), person("Abe", &["a@example.org"])];
        assert_eq!(emails(&suggest(&people, "be", 10)), ["b@example.org", "bt@example.org", "a@example.org"]);
    }

    #[test]
    fn the_limit_applies_after_ranking() {
        let people = [person("Xavier Ada", &["x@example.org"]), person("Ada", &["a@example.org"])];
        assert_eq!(emails(&suggest(&people, "ada", 1)), ["a@example.org"]);
    }

    #[test]
    fn nothing_typed_suggests_nothing() {
        let people = [person("Ada", &["a@example.org"])];
        assert!(suggest(&people, "", 10).is_empty());
        assert!(suggest(&people, "   ", 10).is_empty());
        assert!(suggest(&people, "ada", 0).is_empty());
    }

    #[test]
    fn people_without_addresses_are_not_suggested() {
        let people = [person("Ada", &[]), person("Ada B", &["  "])];
        assert!(suggest(&people, "ada", 10).is_empty());
    }

    #[test]
    fn display_is_the_recipient_field_form() {
        let named = Suggestion { name: "Ada Lovelace".into(), email: "ada@example.org".into() };
        let bare = Suggestion { name: String::new(), email: "ada@example.org".into() };
        assert_eq!(named.display(), "Ada Lovelace <ada@example.org>");
        assert_eq!(bare.display(), "ada@example.org");
    }
}
