use super::*;
use crate::edit::{apply, doc_len, typing, MarkKey, Op};
use crate::model::{Alignment, ListKind, TableCell};

fn bold() -> RunStyle {
    RunStyle { bold: true, ..Default::default() }
}

fn italic() -> RunStyle {
    RunStyle { italic: true, ..Default::default() }
}

/// A document with a heading, a styled run, a list, a link and an image.
fn sample() -> Document {
    let mut d = Document::from_plain_text("Title\nplain bold end\nfirst item\nsee here");
    d.paragraphs[0].style.heading = Some(1);
    d.paragraphs[1].runs = vec![Run::plain("plain "), Run { text: "bold".into(), style: bold() }, Run::plain(" end")];
    d.paragraphs[2].style = ParaStyle { list: ListKind::Bullet, alignment: Alignment::Center, ..Default::default() };
    d.paragraphs[3].runs = vec![
        Run::plain("see "),
        Run { text: "here".into(), style: RunStyle { link: Some("https://gnome.org".into()), ..Default::default() } },
        Run { text: "a dot".into(), style: RunStyle { image: Some("dot.png".into()), ..Default::default() } },
    ];
    d
}

/// Record `op` on the replica and apply it to the local model.
fn edit(r: &mut Replica, d: &mut Document, op: Op) {
    r.record(d, &op).expect("recorded");
    apply(d, &op).expect("applies");
}

#[test]
fn the_document_round_trips_through_the_text() {
    let d = sample();
    let r = Replica::new(1, &d).unwrap();
    assert_eq!(r.paragraphs(), d.paragraphs);
    let (_, forked) = r.fork(2).unwrap();
    assert_eq!(forked, d.paragraphs);
}

/// Two people format overlapping ranges at once: both marks survive, and
/// the overlap has both (Peritext).
#[test]
fn concurrent_bold_and_italic_on_overlapping_ranges_both_survive() {
    let base = Document::from_plain_text("hello wonderful world");
    let mut a = Replica::new(1, &base).unwrap();
    let (mut b, _) = a.fork(2).unwrap();
    let (mut da, mut db) = (base.clone(), base.clone());
    edit(&mut a, &mut da, Op::Mark { start: 0, end: 10, key: MarkKey::Bold, value: bold() });
    edit(&mut b, &mut db, Op::Mark { start: 5, end: 15, key: MarkKey::Italic, value: italic() });
    a.merge_from(&b).unwrap();
    b.merge_from(&a).unwrap();
    let merged = a.paragraphs();
    assert_eq!(merged, b.paragraphs(), "the peers converge");
    let style = |i: usize| {
        let mut pos = 0;
        for r in &merged[0].runs {
            let n = r.text.chars().count();
            if i < pos + n {
                return (r.style.bold, r.style.italic);
            }
            pos += n;
        }
        unreachable!()
    };
    assert_eq!(style(2), (true, false));
    assert_eq!(style(7), (true, true), "the overlap is bold and italic");
    assert_eq!(style(12), (false, true));
    assert_eq!(style(18), (false, false));
}

/// Text inserted at a mark's end while another person applies the mark
/// follows the key's expand rule (ADR 0010, rule 2): bold expands after,
/// a link does not.
#[test]
fn a_concurrent_insert_at_a_marks_end_follows_its_expand_rule() {
    for (key, value, expands) in [
        (MarkKey::Bold, bold(), true),
        (MarkKey::Link, RunStyle { link: Some("https://gnome.org".into()), ..Default::default() }, false),
    ] {
        let base = Document::from_plain_text("hello world");
        let mut a = Replica::new(1, &base).unwrap();
        let (mut b, _) = a.fork(2).unwrap();
        let (mut da, mut db) = (base.clone(), base.clone());
        edit(&mut a, &mut da, Op::Mark { start: 0, end: 5, key, value: value.clone() });
        let op = typing(&db, 5, "X").unwrap();
        edit(&mut b, &mut db, op);
        a.merge_from(&b).unwrap();
        b.merge_from(&a).unwrap();
        let merged = a.paragraphs();
        assert_eq!(merged, b.paragraphs());
        let x = merged[0].runs.iter().find(|r| r.text.contains('X')).unwrap();
        assert_eq!(key.same(&x.style, &value), expands, "{key:?}: {merged:?}");
    }
}

#[test]
fn tables_are_refused_not_encoded_as_text() {
    let mut d = Document::from_plain_text("a\nb");
    d.paragraphs[1].style.table_cell = Some(TableCell { table: 0, row: 0, col: 0 });
    assert_eq!(Replica::new(1, &d).err(), Some(CollabError::Tables));
    let d = Document::from_plain_text("a");
    let mut r = Replica::new(1, &d).unwrap();
    let op = Op::SetParagraphs { para: 0, remove: 1, insert: vec![] };
    assert_eq!(r.record(&d, &op), Err(CollabError::Tables));
}

/// A small deterministic generator (the test must not depend on a seed
/// library to be reproducible).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// A random local edit on `d`: typing, Enter, a delete, bold, italic, a
/// link, or a paragraph style.
fn random_op(rng: &mut Rng, d: &Document) -> Op {
    let len = doc_len(d);
    let at = rng.below(len + 1);
    match rng.below(7) {
        0 | 1 => typing(d, at, ["a", "bc", " ", "xyz"][rng.below(4)]).unwrap(),
        2 => typing(d, at, "\n").unwrap(),
        3 if len > 0 => {
            let at = rng.below(len);
            Op::Delete { at, len: (1 + rng.below(4)).min(len - at) }
        }
        4 => {
            let end = (at + 1 + rng.below(6)).min(len);
            Op::Mark { start: at.min(end), end, key: MarkKey::Bold, value: RunStyle { bold: rng.below(2) == 0, ..Default::default() } }
        }
        5 => {
            let end = (at + 1 + rng.below(6)).min(len);
            let (key, value) = if rng.below(2) == 0 {
                (MarkKey::Italic, italic())
            } else {
                (MarkKey::Link, RunStyle { link: Some("https://example.org".into()), ..Default::default() })
            };
            Op::Mark { start: at.min(end), end, key, value }
        }
        _ => {
            let (p, _) = edit::locate(d, at).unwrap();
            let style = ParaStyle { heading: [None, Some(1), Some(2)][rng.below(3)], ..d.paragraphs[p].style.clone() };
            Op::SetParaStyle { at, style }
        }
    }
}

/// Two peers editing the same document at random, merging now and then:
/// each replica reproduces its own peer's model exactly between merges,
/// and after every exchange both peers show the same document.
#[test]
fn two_peers_editing_at_random_converge() {
    for seed in 1..=12u64 {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed);
        let base = sample();
        let mut a = Replica::new(1, &base).unwrap();
        let (mut b, _) = a.fork(2).unwrap();
        let (mut da, mut db) = (base.clone(), base.clone());
        for round in 0..20 {
            for _ in 0..rng.below(6) {
                let op = random_op(&mut rng, &da);
                if apply(&mut da.clone(), &op).is_ok() {
                    edit(&mut a, &mut da, op);
                }
            }
            for _ in 0..rng.below(6) {
                let op = random_op(&mut rng, &db);
                if apply(&mut db.clone(), &op).is_ok() {
                    edit(&mut b, &mut db, op);
                }
            }
            assert_eq!(a.paragraphs(), da.paragraphs, "seed {seed} round {round}: peer 1's replica is its model");
            assert_eq!(b.paragraphs(), db.paragraphs, "seed {seed} round {round}: peer 2's replica is its model");
            a.merge_from(&b).unwrap();
            b.merge_from(&a).unwrap();
            let (ma, mb) = (a.paragraphs(), b.paragraphs());
            assert_eq!(ma, mb, "seed {seed} round {round}: the peers converge");
            assert!(!ma.is_empty());
            da.paragraphs = ma;
            db.paragraphs = mb;
        }
    }
}
