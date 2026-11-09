//! The address books and contacts of the offline demo.

use super::details::{ContactDetails, GroupFormat, Labeled, Member};
use super::edit::{new_group_vcard, GroupEdit};
use super::read::{local_book, Book};
use crate::i18n::i18n;

/// The books demo mode offers new contacts, the Hylki book among them.
pub(crate) fn demo_books() -> Vec<Book> {
    vec![
        Book {
            uid: "On This Computer".into(),
            name: "On This Computer".into(),
            groups: Some(GroupFormat::Evolution),
        },
        Book {
            uid: "CardDAV — jason@hylki.hyprlab.co".into(),
            name: "CardDAV — jason@hylki.hyprlab.co".into(),
            groups: Some(GroupFormat::Apple),
        },
        local_book(),
    ]
}

/// Sample contacts for demo mode (HYLKI_DEMO): the people from the demo
/// mailbox, fleshed out so the contacts view has something to show off.
/// No EDS identity: demo entries are display-only. The book's name stands
/// in for its UID, so the book filter has something to tell apart.
pub(crate) fn demo_contacts() -> Vec<ContactDetails> {
    let l = |label: &str, value: &str| Labeled {
        label: label.into(),
        value: value.into(),
    };
    let contact = |name: &str, book: &str| ContactDetails {
        name: name.into(),
        book_name: book.into(),
        book_uid: if book == "Hylki Address Book" {
            crate::contacts::local::LOCAL_BOOK_UID.into()
        } else {
            book.into()
        },
        ..ContactDetails::default()
    };
    vec![
        ContactDetails {
            nickname: "Soph".into(),
            org: "Studio.dev".into(),
            title: "Design Lead".into(),
            birthday: "March 4, 1991".into(),
            note: "Prefers Figma links over attachments. Out on Fridays.".into(),
            emails: vec![
                l("Work", "sophie@studio.dev"),
                l("Home", "sophie.t@example.com"),
            ],
            phones: vec![l("Mobile", "+1 (415) 555-0114")],
            addresses: vec![l("Work", "2261 Market St, San Francisco, CA 94114")],
            urls: vec!["sophie.design".into()],
            ..contact("Sophie Turner", "CardDAV — jason@hylki.hyprlab.co")
        },
        ContactDetails {
            org: "Studio.dev".into(),
            title: "Engineering Manager".into(),
            emails: vec![l("Work", "marcus@studio.dev")],
            phones: vec![
                l("Mobile", "+1 (415) 555-0187"),
                l("Work", "+1 (415) 555-0100"),
            ],
            ..contact("Marcus Chen", "CardDAV — jason@hylki.hyprlab.co")
        },
        ContactDetails {
            org: "Studio.dev".into(),
            title: "Product Designer".into(),
            birthday: "November 19".into(),
            emails: vec![l("Work", "priya@studio.dev")],
            phones: vec![l("Mobile", "+1 (628) 555-0163")],
            ..contact("Priya Sharma", "Google — jason.m@gmail.com")
        },
        ContactDetails {
            title: "Illustrator".into(),
            note: "Freelance — invoices go to the studio address.".into(),
            emails: vec![l("", "emma@example.com")],
            urls: vec!["emmawright.art".into()],
            ..contact("Emma Wright", "Google — jason.m@gmail.com")
        },
        ContactDetails {
            org: "GNOME Foundation".into(),
            emails: vec![l("Work", "lena@gnome.org")],
            addresses: vec![l("Work", "21 Orinda Way, Orinda, CA 94563")],
            ..contact("Lena Fischer", "On This Computer")
        },
        ContactDetails {
            org: "Ferrous Type".into(),
            title: "Founder".into(),
            emails: vec![
                l("Work", "diego@ferroustype.com"),
                l("Home", "d.alvarez@example.com"),
            ],
            phones: vec![l("Mobile", "+34 612 555 021")],
            urls: vec!["ferroustype.com".into()],
            ..contact("Diego Álvarez", "CardDAV — jason@hylki.hyprlab.co")
        },
        ContactDetails {
            emails: vec![l("Home", "tom.okafor@example.com")],
            phones: vec![l("Mobile", "+44 7700 900 214")],
            birthday: "July 30, 1988".into(),
            ..contact("Tom Okafor", "Hylki Address Book")
        },
        ContactDetails {
            org: "Kim & Partners".into(),
            title: "Attorney".into(),
            emails: vec![l("Work", "grace@kimpartners.example")],
            phones: vec![l("Work", "+1 (212) 555-0170")],
            addresses: vec![l("Work", "425 Lexington Ave, New York, NY 10017")],
            ..contact("Grace Kim", "CardDAV — jason@hylki.hyprlab.co")
        },
        demo_group(
            "Studio.dev Team",
            false,
            &[
                ("Sophie Turner", "sophie@studio.dev"),
                ("Marcus Chen", "marcus@studio.dev"),
                ("Priya Sharma", "priya@studio.dev"),
            ],
        ),
        demo_group(
            "Book Club",
            true,
            &[
                ("Emma Wright", "emma@example.com"),
                ("Tom Okafor", "tom.okafor@example.com"),
                ("Grace Kim", "grace@kimpartners.example"),
                ("Lena Fischer", "lena@gnome.org"),
            ],
        ),
    ]
}

/// A demo group in the Hylki book, with a card to edit.
fn demo_group(name: &str, hide_addresses: bool, people: &[(&str, &str)]) -> ContactDetails {
    let members: Vec<Member> = people
        .iter()
        .map(|(n, e)| Member {
            name: n.to_string(),
            email: e.to_string(),
            ..Member::default()
        })
        .collect();
    let edit = GroupEdit {
        name: name.into(),
        members: members.clone(),
        hide_addresses,
        kept: Vec::new(),
    };
    ContactDetails {
        name: name.into(),
        book_name: i18n("Hylki Address Book"),
        book_uid: crate::contacts::local::LOCAL_BOOK_UID.into(),
        group: Some(GroupFormat::Evolution),
        members,
        hide_addresses,
        raw_vcard: new_group_vcard(&edit),
        // A group is told apart by its UID, as a real one is.
        eds_uid: format!("demo-{name}"),
        ..ContactDetails::default()
    }
}
