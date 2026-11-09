//! The vCards Hylki reads and writes, pinned: what a corpus of cards parses
//! to, and the exact text of every card the editors produce.

use super::*;
use crate::config::golden::assert_golden;

/// Cards in the shapes the address books hand over: Evolution, Apple and
/// vCard 4 groups, grouped properties, quoted parameters, folded lines,
/// escapes, and a card with almost nothing in it.
const CORPUS: &[(&str, &str)] = &[
    (
        "person",
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:p1\r\nFN:Pat Doe\r\nN:Doe;Pat;;;\r\n\
         NICKNAME:Patty\r\nORG:Acme\\, Inc.;Sales\r\nTITLE:Boss\r\nNOTE:Line one\\nline two\r\n\
         EMAIL;TYPE=WORK:pat@work.example\r\nEMAIL;TYPE=HOME,INTERNET:pat@home.example\r\n\
         TEL;TYPE=CELL:+1 555 0100\r\nTEL;TYPE=FAX:+1 555 0101\r\nURL:https://pat.example\r\n\
         ADR;TYPE=HOME:PO 7;Flat 2;1 Main St\\, Rear;Springfield;IL;62701;USA\r\n\
         BDAY:1985-04-12\r\nX-CUSTOM:kept\r\nEND:VCARD\r\n",
    ),
    (
        "grouped-properties",
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Grouped\r\nitem1.EMAIL;type=INTERNET:g@x.example\r\n\
         item1.X-ABLabel:Other\r\nitem2.TEL:+44 20 7946 0000\r\nEND:VCARD\r\n",
    ),
    (
        "quoted-parameters",
        "BEGIN:VCARD\r\nVERSION:4.0\r\nFN;ALTID=\"a:b\":Quoted: Name\r\n\
         EMAIL;PID=\"1:2\";TYPE=work:q@x.example\r\n\
         PHOTO;VALUE=uri:https://remote.example/p.png\r\nEND:VCARD\r\n",
    ),
    (
        "folded",
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Folded\r\n  Name\r\nEMAIL:fold@x.exa\r\n mple\r\n\
         BDAY:--04-12\r\nEND:VCARD\r\n",
    ),
    (
        "evolution-list",
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:l1\r\nFN:Team\r\nX-EVOLUTION-LIST:TRUE\r\n\
         X-EVOLUTION-LIST-SHOW-ADDRESSES:FALSE\r\n\
         EMAIL;X-EVOLUTION-DEST-CONTACT-UID=p1;X-EVOLUTION-DEST-NAME=Pat Doe:Pat Doe <pat@work.example>\r\n\
         EMAIL;X-EVOLUTION-DEST-EMAIL=ann@x.example;X-EVOLUTION-DEST-NAME=Ann:not an address\r\n\
         EMAIL:bare@x.example\r\nEND:VCARD\r\n",
    ),
    (
        "apple-group",
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:g1\r\nFN:Friends\r\nN:Friends\r\n\
         X-ADDRESSBOOKSERVER-KIND:group\r\n\
         X-ADDRESSBOOKSERVER-MEMBER:urn:uuid:p1\r\nX-ADDRESSBOOKSERVER-MEMBER:urn:uuid:missing\r\nEND:VCARD\r\n",
    ),
    (
        "kind-group",
        "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:k1\r\nFN:Club\r\nKIND:group\r\n\
         MEMBER:urn:uuid:p1\r\nMEMBER:mailto:club@x.example\r\nEND:VCARD\r\n",
    ),
    (
        "nearly-empty",
        "BEGIN:VCARD\r\nVERSION:3.0\r\nTEL:+1 555 0199\r\nEND:VCARD\r\n",
    ),
    ("not-a-card", "hello"),
];

#[test]
fn the_corpus_parses_as_it_did() {
    let _style = crate::datefmt::TEST_STYLE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut out = String::new();
    for (name, card) in CORPUS {
        out += &format!(
            "== {name}\ndisplay name: {:?}\ngroup: {}\n{:#?}\n",
            vcard_display_name(card),
            is_group_vcard(card),
            parse_vcard_fields(card),
        );
    }
    // How a birthday is written follows the locale and the date style, which
    // are process-wide and settle on whatever the first date formatted in the
    // test run saw. The golden file pins where the formatted birthday goes;
    // the formatting itself is datefmt's and is compared live.
    for raw in ["1985-04-12", "--04-12"] {
        let shown = format!("birthday: {:?},", pretty_birthday(raw));
        out = out.replace(&shown, &format!("birthday: <pretty_birthday({raw:?})>,"));
    }
    assert_golden("contacts/testdata/parsed.txt", &out);
}

fn person_edit() -> ContactEdit {
    ContactEdit {
        name: "Pat Q. Doe".into(),
        nickname: "P, the; one".into(),
        org: "Acme".into(),
        title: "Chief\\Boss".into(),
        note: "Two\nlines\r\n".into(),
        emails: vec![
            Labeled {
                label: "Work".into(),
                value: "pat@work.example".into(),
            },
            Labeled {
                label: "Custom".into(),
                value: "pat@other.example".into(),
            },
        ],
        phones: vec![Labeled {
            label: "Mobile".into(),
            value: "+1 555 0100".into(),
        }],
        urls: vec!["https://pat.example".into()],
        addresses: vec![Postal {
            label: "Home".into(),
            street: "1 Main St, Rear".into(),
            locality: "Shelbyville".into(),
            ..Default::default()
        }],
        birthday: birthday_value(Some(1985), 4, 12),
        photo: PhotoEdit::Set(vec![0x89, b'P', b'N', b'G']),
    }
}

#[test]
fn the_editors_write_these_cards() {
    let mut out = String::new();
    let mut show = |name: &str, card: String| out += &format!("== {name}\n{card}");
    show("patched person", patched_vcard(CORPUS[0].1, &person_edit()));
    show("new person", new_vcard(&person_edit()));
    let members = vec![
        Member {
            name: "Pat Doe".into(),
            email: "pat@work.example".into(),
            ..Default::default()
        },
        Member {
            name: String::new(),
            email: "new@x.example".into(),
            ..Default::default()
        },
    ];
    let group = GroupEdit {
        name: "Team, renamed".into(),
        members: members.clone(),
        hide_addresses: true,
        kept: vec!["EMAIL;X-HIDDEN=1:hidden@x.example".into()],
    };
    show("new group", new_group_vcard(&group));
    show(
        "patched evolution list",
        patched_group_vcard(CORPUS[4].1, &group),
    );
    let apple = GroupEdit {
        name: "Friends".into(),
        members: vec![Member {
            name: "Pat Doe".into(),
            email: "pat@work.example".into(),
            uid: "p1".into(),
            ..Default::default()
        }],
        hide_addresses: false,
        kept: Vec::new(),
    };
    show(
        "patched apple group",
        patched_apple_group(CORPUS[5].1, &apple),
    );
    show("person for an address", vcard_for("  ", "who@x.example"));
    show("person with a name", vcard_for(" Ann ", "ann@x.example"));
    show(
        "address added",
        add_email_to_vcard(CORPUS[7].1, "added@x.example"),
    );
    show(
        "line after the version",
        insert_after_version(CORPUS[7].1, "UID:new"),
    );
    let details = parse_vcard_fields(CORPUS[0].1).unwrap();
    show("moved card", moved_vcard(&details));
    show("empty card", EMPTY_VCARD.to_string());
    assert_golden("contacts/testdata/written.txt", &out);
}

#[test]
fn values_escape_and_unescape() {
    for text in ["plain", "a,b;c\\d", "two\nlines", "trailing\\"] {
        let escaped = escape_vcard_value(text);
        assert_eq!(unescape_vcard_text(&escaped), text, "{escaped}");
    }
    assert_eq!(escape_vcard_value("a\r\nb"), "a\\nb");
    assert_eq!(unescape_vcard_text("a\\Nb\\,c"), "a\nb,c");
    assert_eq!(
        split_vcard_line("FN;X=\"a:b\":v:w"),
        Some(("FN;X=\"a:b\"", "v:w"))
    );
    assert_eq!(split_vcard_line("no colon"), None);
    assert_eq!(unfold_vcard("A:1\r\n 2\r\n\t3\r\nB:4"), "A:123\r\nB:4");
}
