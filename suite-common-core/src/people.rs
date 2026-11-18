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

/// The person one vCard describes, as an address book hands it over (what
/// Evolution Data Server stores and returns): the formatted name (`FN`, or
/// the structured `N` when there is none) and every `EMAIL`. `None` for a
/// card that has no address to offer, and for a contact *group* (`X-EVOLUTION-LIST`
/// or `KIND:group`), which is a list of people and not one.
///
/// Folded lines (a continuation starts with a space or tab) are joined first,
/// parameters (`EMAIL;TYPE=work:...`, `item1.EMAIL:...`) are ignored, and the
/// `\,` `\;` `\\` and `\n` escapes are undone.
pub fn from_vcard(vcard: &str) -> Option<Person> {
    let mut lines: Vec<String> = Vec::new();
    for raw in vcard.lines() {
        match (raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')), lines.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ => lines.push(raw.to_string()),
        }
    }
    let unescape = |v: &str| {
        let mut out = String::new();
        let mut chars = v.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        }
        out.trim().to_string()
    };
    let mut full_name = String::new();
    let mut structured = String::new();
    let mut emails = Vec::new();
    for line in &lines {
        let Some((head, value)) = line.split_once(':') else { continue };
        // "item1.EMAIL;TYPE=work" -> "EMAIL"
        let name = head.split(';').next().unwrap_or("");
        let name = name.rsplit('.').next().unwrap_or(name).to_ascii_uppercase();
        match name.as_str() {
            "FN" if full_name.is_empty() => full_name = unescape(value),
            "N" if structured.is_empty() => {
                // family;given;additional;prefix;suffix -> "given additional family"
                let parts: Vec<&str> = value.split(';').collect();
                let get = |i: usize| parts.get(i).map(|p| unescape(p)).unwrap_or_default();
                structured = [get(1), get(2), get(0)].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
            }
            "EMAIL" => {
                let email = unescape(value);
                if !email.is_empty() {
                    emails.push(email);
                }
            }
            "KIND" if value.trim().eq_ignore_ascii_case("group") => return None,
            "X-EVOLUTION-LIST" => return None,
            _ => {}
        }
    }
    if emails.is_empty() {
        return None;
    }
    let name = if full_name.is_empty() { structured } else { full_name };
    Some(Person { name, emails })
}

/// The Evolution Data Server query for every contact with an address: the
/// picker has no use for the others, and a book of thousands is cheaper to
/// read without them.
pub const EDS_QUERY_WITH_EMAIL: &str = "(exists \"email\")";

/// The newest of the versioned bus names `names` holds for `prefix`: EDS
/// suffixes its interfaces with a number that grows across releases
/// (`org.gnome.evolution.dataserver.AddressBook10`, `…Sources5`), so the
/// name has to be looked up, and the highest wins when several are listed.
pub fn eds_bus_name(names: &[String], prefix: &str) -> Option<String> {
    names
        .iter()
        .filter_map(|name| Some((name.strip_prefix(prefix)?.parse::<u32>().ok()?, name)))
        .max_by_key(|(version, _)| *version)
        .map(|(_, name)| name.clone())
}

/// The UIDs of the address books among the EDS registry's sources, given as
/// (UID, key file): a source with an `[Address Book]` group, unless its
/// `[Data Source]` group says `Enabled=false` (what GNOME Online Accounts
/// writes when Contacts is switched off for an account). Only that group's
/// `Enabled` counts; other groups have keys of the same name.
pub fn eds_address_books(sources: &[(String, String)]) -> Vec<String> {
    sources
        .iter()
        .filter(|(_, data)| {
            let mut group = "";
            let mut is_book = false;
            let mut disabled = false;
            for line in data.lines().map(str::trim) {
                if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                    group = name;
                    is_book |= name == "Address Book";
                } else if group == "Data Source" {
                    if let Some((key, value)) = line.split_once('=') {
                        if key.trim() == "Enabled" && value.trim().eq_ignore_ascii_case("false") {
                            disabled = true;
                        }
                    }
                }
            }
            is_book && !disabled
        })
        .map(|(uid, _)| uid.clone())
        .collect()
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
    #[test]
    fn a_vcard_gives_its_name_and_every_address() {
        let card = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada Lovelace\r\nN:Lovelace;Ada;;;\r\nEMAIL;TYPE=HOME:ada@home.example\r\nitem1.EMAIL;TYPE=WORK:ada@work.example\r\nEND:VCARD\r\n";
        assert_eq!(from_vcard(card), Some(person("Ada Lovelace", &["ada@home.example", "ada@work.example"])));
    }

    #[test]
    fn a_card_without_a_formatted_name_uses_the_structured_one() {
        let card = "BEGIN:VCARD\nN:Hopper;Grace;Brewster;;\nEMAIL:grace@example.org\nEND:VCARD\n";
        assert_eq!(from_vcard(card).unwrap().name, "Grace Brewster Hopper");
    }

    #[test]
    fn folded_lines_and_escapes_are_undone() {
        let card = "BEGIN:VCARD\nFN:Lovelace\\, Augusta\n Ada\nEMAIL:ada@\n example.org\nEND:VCARD\n";
        let person = from_vcard(card).unwrap();
        assert_eq!(person.name, "Lovelace, AugustaAda");
        assert_eq!(person.emails, ["ada@example.org"]);
    }

    #[test]
    fn a_card_with_no_address_and_a_group_are_not_people_to_pick() {
        assert_eq!(from_vcard("BEGIN:VCARD\nFN:No Mail\nEND:VCARD\n"), None);
        assert_eq!(from_vcard("BEGIN:VCARD\nFN:Team\nKIND:group\nEMAIL:team@example.org\nEND:VCARD\n"), None);
        assert_eq!(from_vcard("BEGIN:VCARD\nFN:Team\nX-EVOLUTION-LIST:TRUE\nEMAIL:team@example.org\nEND:VCARD\n"), None);
    }
    #[test]
    fn the_newest_eds_interface_is_used() {
        let names: Vec<String> = ["org.gnome.evolution.dataserver.AddressBook9", "org.freedesktop.DBus",
            "org.gnome.evolution.dataserver.AddressBook10", "org.gnome.evolution.dataserver.AddressBookX"]
            .iter().map(|s| s.to_string()).collect();
        assert_eq!(
            eds_bus_name(&names, "org.gnome.evolution.dataserver.AddressBook").as_deref(),
            Some("org.gnome.evolution.dataserver.AddressBook10")
        );
        assert_eq!(eds_bus_name(&names, "org.gnome.evolution.dataserver.Sources"), None);
    }

    #[test]
    fn only_enabled_address_books_are_read() {
        let sources = [
            ("system-address-book".to_string(), "[Data Source]\nDisplayName=Personal\nEnabled=true\n\n[Address Book]\nBackendName=local\n".to_string()),
            ("google-off".to_string(), "[Data Source]\nDisplayName=Contacts\nEnabled=false\n\n[Address Book]\nBackendName=google\n".to_string()),
            ("calendar".to_string(), "[Data Source]\nDisplayName=Calendar\n\n[Calendar]\nBackendName=local\n".to_string()),
            // Enabled=false in another group does not switch the book off.
            ("carddav".to_string(), "[Data Source]\nDisplayName=Work\n\n[Address Book]\nBackendName=carddav\n\n[Refresh]\nEnabled=false\n".to_string()),
        ];
        assert_eq!(eds_address_books(&sources), ["system-address-book", "carddav"]);
    }
}
