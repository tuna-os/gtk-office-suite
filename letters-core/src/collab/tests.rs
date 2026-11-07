use super::rebase::it_lists;
use super::*;
use crate::chips::{Chip, ChipKind};
use crate::edit::{apply, doc_len, sequence_text, typing};
use crate::model::{Comment, DocSection, ListKind, ParagraphRange, RevisionKind, TableCell};
use loro::LoroDoc;

fn bold() -> RunStyle {
    RunStyle { bold: true, ..Default::default() }
}

fn italic() -> RunStyle {
    RunStyle { italic: true, ..Default::default() }
}

fn link() -> RunStyle {
    RunStyle { link: Some("https://gnome.org".into()), ..Default::default() }
}

fn page() -> PageGeometry {
    PageGeometry {
        width_pt: 595.3,
        height_pt: 841.9,
        margin_top_pt: 72.0 / 3.0,
        margin_bottom_pt: 72.0,
        margin_left_pt: 90.0,
        margin_right_pt: 90.0,
        columns: 1,
        column_gap_pt: 36.0,
        header_pt: None,
        footer_pt: None,
        border: None,
    }
}

fn revision(kind: RevisionKind) -> Revision {
    Revision { kind, author: "Ada".into(), date: "2026-10-26T10:00:00Z".into(), under: None }
}

/// A document with a heading, a styled run, a list, a link and an image.
fn sample() -> Document {
    let mut d = Document::from_plain_text("Title\nplain bold end\nfirst item\nsee here");
    d.paragraphs[0].style.heading = Some(1);
    d.paragraphs[1].runs = vec![Run::plain("plain "), Run { text: "bold".into(), style: bold() }, Run::plain(" end")];
    d.paragraphs[2].style = ParaStyle { list: ListKind::Bullet, alignment: Alignment::Center, ..Default::default() };
    d.paragraphs[3].runs = vec![
        Run::plain("see "),
        Run { text: "here".into(), style: link() },
        Run { text: "a dot".into(), style: RunStyle { image: Some("dot.png".into()), ..Default::default() } },
    ];
    d
}

/// `sample` with every kind of thing the encoding carries: a page, floats
/// that must come back bit for bit, character attributes that have no
/// `MarkKey`, a tracked insertion, a chip and a tracked deletion of it.
fn rich() -> Document {
    let mut d = sample();
    d.page = Some(page());
    d.paragraphs[0].style.space_before_pt = 0.1 + 0.2;
    d.paragraphs[0].style.left_indent_pt = 1.0 / 3.0;
    d.paragraphs[0].style.line_spacing = 1.15;
    d.paragraphs[1].runs.push(Run {
        text: " shouted".into(),
        style: RunStyle {
            caps: true,
            regular: true,
            letter_spacing_tw: Some(-20),
            position_hp: Some(4),
            font_size_hp: Some(24),
            color: Some("FF0000".into()),
            font_family: Some("Liberation Serif".into()),
            vert_align: Some(VertAlign::Superscript),
            strikethrough: true,
            underline: true,
            highlight: true,
            ..Default::default()
        },
    });
    d.paragraphs[1].runs.push(Run { text: " added".into(), style: RunStyle { revision: Some(revision(RevisionKind::Insert)), ..Default::default() } });
    d.paragraphs[3].runs.push(Run {
        text: "26 Oct 2026".into(),
        style: RunStyle {
            chip: Some(Chip { kind: ChipKind::Date, value: "2026-10-26".into() }),
            revision: Some(revision(RevisionKind::Delete)),
            ..Default::default()
        },
    });
    d
}

/// One editor: its model, its replica and its history, in step.
struct Peer {
    replica: Replica,
    doc: Document,
    history: History,
}

/// Two peers on the same document.
fn peers(base: &Document) -> (Peer, Peer) {
    let a = Replica::new(1, base).unwrap();
    let b = a.fork(2).unwrap();
    let peer = |replica| Peer { replica, doc: base.clone(), history: History::default() };
    (peer(a), peer(b))
}

impl Peer {
    /// A local op: recorded, applied and put on the history, as the shell
    /// does. Returns the update that carries it.
    fn edit(&mut self, op: Op) -> Vec<u8> {
        let version = self.replica.version_bytes();
        self.replica.record(&self.doc, &op).expect("recorded");
        let inverse = apply(&mut self.doc, &op).expect("applies");
        self.history.record(inverse);
        self.replica.export_since(&version).unwrap()
    }

    fn type_text(&mut self, at: usize, text: &str) -> Vec<u8> {
        let op = typing(&self.doc, at, text).unwrap();
        self.edit(op)
    }

    /// An update from a peer, as the shell takes it in.
    fn receive(&mut self, update: &[u8]) -> ImportReport {
        let report = self.replica.import_updates(update).expect("imported");
        report.rebase(&mut self.history);
        self.doc = self.replica.rebuild(&self.doc).unwrap();
        report
    }

    /// Undo or redo one step; the update that carries its ops, or `None`
    /// when there was no step or it was dropped.
    fn walk(&mut self, redo: bool) -> Option<Vec<u8>> {
        let version = self.replica.version_bytes();
        let mut replay = self.doc.clone();
        let ops = (if redo { self.history.redo(&mut self.doc) } else { self.history.undo(&mut self.doc) })?;
        for op in &ops {
            self.replica.record(&replay, op).expect("an undo is an ordinary op");
            apply(&mut replay, op).expect("and applies to the model");
        }
        assert_eq!(replay, self.doc);
        Some(self.replica.export_since(&version).unwrap())
    }

    fn undo(&mut self) -> Option<Vec<u8>> {
        self.walk(false)
    }

    fn redo(&mut self) -> Option<Vec<u8>> {
        self.walk(true)
    }

    fn text(&self) -> String {
        sequence_text(&self.doc)
    }

    /// The replica shows exactly the model.
    fn assert_in_step(&self, what: &str) {
        assert_eq!(self.replica.paragraphs().unwrap(), self.doc.paragraphs, "{what}: the replica is not the model");
    }
}

/// What `a` has that `b` lacks, as one update.
fn since(a: &Peer, b: &Peer) -> Vec<u8> {
    a.replica.export_since(&b.replica.version_bytes()).unwrap()
}

/// Both peers take in what the other has, from the same instant.
fn exchange(a: &mut Peer, b: &mut Peer) {
    let (to_b, to_a) = (since(a, b), since(b, a));
    b.receive(&to_b);
    a.receive(&to_a);
}

fn permutations(n: usize) -> Vec<Vec<usize>> {
    fn go(prefix: &mut Vec<usize>, left: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if left.is_empty() {
            out.push(prefix.clone());
            return;
        }
        for i in 0..left.len() {
            let x = left.remove(i);
            prefix.push(x);
            go(prefix, left, out);
            prefix.pop();
            left.insert(i, x);
        }
    }
    let mut out = Vec::new();
    go(&mut Vec::new(), &mut (0..n).collect(), &mut out);
    out
}

// ------------------------------------------------------------------ the encoding

#[test]
fn the_document_round_trips_through_the_text() {
    for d in [sample(), rich()] {
        let r = Replica::new(1, &d).unwrap();
        assert_eq!(r.paragraphs().unwrap(), d.paragraphs);
        assert_eq!(r.rebuild(&d).unwrap(), d, "the page and the floats come back exactly");
        assert_eq!(r.fork(2).unwrap().rebuild(&d).unwrap(), d);
        let (joined, document) = Replica::from_snapshot(3, &r.export_snapshot().unwrap(), &d).unwrap();
        assert_eq!(document, d);
        assert_eq!(joined.paragraphs().unwrap(), d.paragraphs);
        let (_, replayed) = Replica::from_updates(4, &r.export_all().unwrap(), &d).unwrap();
        assert_eq!(replayed, d);
    }
}

#[test]
fn a_tracked_change_on_an_object_is_a_mark_that_ops_reach() {
    let mut d = sample();
    let mut r = Replica::new(1, &d).unwrap();
    let at = doc_len(&d) - 1;
    assert!(sequence_text(&d).ends_with('\u{FFFC}'), "the document ends in an object");
    for value in [Some(revision(RevisionKind::Delete)), None] {
        let op = Op::Mark { start: at, end: at + 1, key: MarkKey::Revision, value: RunStyle { revision: value, ..Default::default() } };
        r.record(&d, &op).unwrap();
        apply(&mut d, &op).unwrap();
        assert_eq!(r.paragraphs().unwrap(), d.paragraphs);
    }
}

#[test]
fn the_page_is_one_last_writer_wins_value() {
    let base = sample();
    let (mut a, mut b) = peers(&base);
    let to_b = a.edit(Op::SetPage { page: Some(page()) });
    b.receive(&to_b);
    assert_eq!(b.doc.page, Some(page()));
    let to_a = b.edit(Op::SetPage { page: None });
    let report = a.receive(&to_a);
    assert!(report.page);
    assert_eq!(a.doc.page, None);
}

// ------------------------------------------------------------------ convergence

/// Two people typing in one paragraph, three keystrokes each, at the same
/// place: whatever order the six updates arrive in, a replica that sees them
/// all shows the same document, byte for byte (its serialization and its
/// version), and neither person's word is split by the other's.
#[test]
fn two_replicas_typing_in_one_paragraph_converge_in_every_delivery_order() {
    let base = Document::from_plain_text("hello world");
    let (mut a, mut b) = peers(&base);
    let seed = a.replica.fork(3).unwrap();
    let mut updates = Vec::new();
    for i in 0..3 {
        updates.push(a.type_text(5 + i, "A"));
    }
    for i in 0..3 {
        updates.push(b.type_text(5 + i, "B"));
    }
    eprintln!("one keystroke is {} bytes of update", updates[0].len());

    let mut reference: Option<(Vec<u8>, Vec<u8>)> = None;
    let mut pending_seen = false;
    for order in permutations(updates.len()) {
        let mut r = seed.fork(99).unwrap();
        for &i in &order {
            pending_seen |= r.import_updates(&updates[i]).unwrap().pending;
        }
        let document = serde_json::to_vec(&r.paragraphs().unwrap()).unwrap();
        let text = sequence_text(&r.rebuild(&base).unwrap());
        assert!(text == "helloAAABBB world" || text == "helloBBBAAA world", "order {order:?}: {text}");
        let version = r.version_bytes();
        // The snapshot's bytes are not the same from one order to the next
        // (measured: its header and checksum differ), so what is compared is
        // the document a joiner builds from it.
        let (_, joined) = Replica::from_snapshot(100, &r.export_snapshot().unwrap(), &base).unwrap();
        assert_eq!(serde_json::to_vec(&joined.paragraphs).unwrap(), document, "order {order:?}: a joiner's document differs");
        match &reference {
            None => reference = Some((document, version)),
            Some((want_document, want_version)) => {
                assert_eq!(&document, want_document, "order {order:?}: not byte for byte the same document");
                assert_eq!(&version, want_version, "order {order:?}: not the same version");
            }
        }
    }
    assert!(pending_seen, "some orders deliver a keystroke before the one it follows");

    // The two people themselves, taking each other's updates.
    for u in &updates[3..] {
        a.receive(u);
    }
    for u in &updates[..3] {
        b.receive(u);
    }
    assert_eq!(a.doc.paragraphs, b.doc.paragraphs);
    let want: Vec<Paragraph> = serde_json::from_slice(&reference.unwrap().0).unwrap();
    assert_eq!(a.doc.paragraphs, want);
}

/// A bold applied by one person while another types at its end. Measured on
/// the Loro in `Cargo.lock` (the test is that measurement): the text typed
/// concurrently is not taken into the mark, for a mark that expands after
/// (bold) as for one that does not (a link), and both peers agree. ADR 0010's
/// rule 2 governs text typed after a mark that exists on both sides: there the
/// model's `typing_style` writes the style into the op, and Loro agrees (the
/// next test). Between a mark and an insert made at once, the insert stays
/// unmarked: a Loro upgrade that changes it shows up here.
#[test]
fn a_concurrent_insert_at_a_marks_end_stays_unmarked() {
    for (key, value) in [(MarkKey::Bold, bold()), (MarkKey::Link, link())] {
        let base = Document::from_plain_text("hello world");
        let (mut a, mut b) = peers(&base);
        let to_b = a.edit(Op::Mark { start: 0, end: 5, key, value: value.clone() });
        let to_a = b.type_text(5, "X");
        b.receive(&to_b);
        a.receive(&to_a);
        assert_eq!(a.doc.paragraphs, b.doc.paragraphs, "{key:?}: the peers converge");
        let x = a.doc.paragraphs[0].runs.iter().find(|r| r.text.contains('X')).unwrap();
        assert!(!key.same(&x.style, &value), "{key:?}: {:?}", a.doc.paragraphs);
        assert_eq!(a.text(), "helloX world");
        let hello = &a.doc.paragraphs[0].runs[0];
        assert!(key.same(&hello.style, &value) && hello.text.starts_with("hello"), "{key:?}: the mark itself survives");
    }
}

/// Text typed at the end of a mark both peers already have follows the
/// mark's expand rule, concurrently too: bold continues, a link does not.
#[test]
fn concurrent_typing_at_the_end_of_an_existing_mark_follows_its_expand_rule() {
    for (key, value, expands) in [(MarkKey::Bold, bold(), true), (MarkKey::Link, link(), false)] {
        let mut base = Document::from_plain_text("hello world");
        base.paragraphs[0].runs = vec![Run { text: "hello".into(), style: value.clone() }, Run::plain(" world")];
        let (mut a, mut b) = peers(&base);
        let to_b = a.type_text(5, "X");
        let to_a = b.type_text(5, "Y");
        b.receive(&to_b);
        a.receive(&to_a);
        assert_eq!(a.doc.paragraphs, b.doc.paragraphs, "{key:?}: the peers converge");
        for letter in ['X', 'Y'] {
            let run = a.doc.paragraphs[0].runs.iter().find(|r| r.text.contains(letter)).unwrap();
            assert_eq!(key.same(&run.style, &value), expands, "{key:?} {letter}: {:?}", a.doc.paragraphs);
        }
    }
}

/// Two people format overlapping ranges at once: both marks survive, and
/// the overlap has both (Peritext).
#[test]
fn concurrent_bold_and_italic_on_overlapping_ranges_both_survive() {
    let base = Document::from_plain_text("hello wonderful world");
    let (mut a, mut b) = peers(&base);
    let to_b = a.edit(Op::Mark { start: 0, end: 10, key: MarkKey::Bold, value: bold() });
    let to_a = b.edit(Op::Mark { start: 5, end: 15, key: MarkKey::Italic, value: italic() });
    b.receive(&to_b);
    a.receive(&to_a);
    assert_eq!(a.doc.paragraphs, b.doc.paragraphs);
    let style = |i: usize| {
        let mut pos = 0;
        for r in &a.doc.paragraphs[0].runs {
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

#[test]
fn enter_and_a_restyle_in_different_paragraphs_converge() {
    let base = sample();
    let (mut a, mut b) = peers(&base);
    let to_b = a.type_text(8, "\n");
    let style = ParaStyle { heading: Some(2), ..base.paragraphs[2].style.clone() };
    let to_a = b.edit(Op::SetParaStyle { at: edit::paragraph_start(&base, 2), style });
    b.receive(&to_b);
    a.receive(&to_a);
    assert_eq!(a.doc.paragraphs, b.doc.paragraphs);
    assert_eq!(a.doc.paragraphs.len(), base.paragraphs.len() + 1);
    assert_eq!(a.doc.paragraphs[3].style.heading, Some(2), "the restyled paragraph, now the fourth");
}

// ------------------------------------------------------------------ deltas, versions, joins

/// Taking each change as its own update gives the document that replaying
/// the whole history gives, at every step.
#[test]
fn delta_updates_equal_a_replay() {
    let base = rich();
    let (mut a, mut b) = peers(&base);
    let mut follower = a.replica.fork(7).unwrap();
    let mut rng = Rng(0xD311A);
    let mut follower_doc = base.clone();
    for step in 0..60 {
        let op = random_op(&mut rng, &a.doc);
        if apply(&mut a.doc.clone(), &op).is_err() {
            continue;
        }
        let update = a.edit(op);
        follower.import_updates(&update).unwrap();
        follower_doc = follower.rebuild(&follower_doc).unwrap();
        assert_eq!(follower_doc.paragraphs, a.doc.paragraphs, "step {step}: the follower is behind");
        let (_, replay) = Replica::from_updates(8, &a.replica.export_all().unwrap(), &base).unwrap();
        assert_eq!(replay.paragraphs, a.doc.paragraphs, "step {step}: a replay differs");
    }
    // A second author, merged through one update.
    for _ in 0..20 {
        let op = random_op(&mut rng, &b.doc);
        if apply(&mut b.doc.clone(), &op).is_ok() {
            b.edit(op);
        }
    }
    let to_a = since(&b, &a);
    let to_follower = since(&b, &a);
    a.receive(&to_a);
    follower.import_updates(&to_follower).unwrap();
    assert_eq!(follower.rebuild(&base).unwrap().paragraphs, a.doc.paragraphs);
    // Importing again changes nothing.
    let again = a.replica.import_updates(&to_a).unwrap();
    assert!(!again.changed() && again.edits.is_empty(), "{again:?}");
}

#[test]
fn a_replica_that_joins_by_snapshot_edits_and_converges() {
    let base = sample();
    let (mut a, _) = peers(&base);
    a.type_text(5, "!");
    a.edit(Op::Mark { start: 0, end: 5, key: MarkKey::Italic, value: italic() });
    let snapshot = a.replica.export_snapshot().unwrap();

    // The joiner opened the file (`base`), then takes the snapshot.
    let (replica, document) = Replica::from_snapshot(5, &snapshot, &base).unwrap();
    assert_eq!(document.paragraphs, a.doc.paragraphs);
    let mut joiner = Peer { replica, doc: document, history: History::default() };
    let to_a = joiner.type_text(0, "Joined: ");
    a.receive(&to_a);
    assert_eq!(a.doc.paragraphs, joiner.doc.paragraphs);
    assert!(a.text().starts_with("Joined: Title!"));

    // And the other way: the joiner takes what A does next, from its version.
    let to_joiner = a.type_text(a.text().chars().count(), "?");
    joiner.receive(&to_joiner);
    assert_eq!(a.doc.paragraphs, joiner.doc.paragraphs);
}

/// Two replicas in one process take each other's changes through
/// `merge_from`, which is an `import_updates` of the delta: the dry run
/// applies, and the report says what moved.
#[test]
fn replicas_in_one_process_merge_through_the_dry_run() {
    let base = sample();
    let (mut a, mut b) = peers(&base);
    a.type_text(5, "!");
    b.edit(Op::Mark { start: 0, end: 5, key: MarkKey::Italic, value: italic() });
    let into_a = a.replica.merge_from(&b.replica).unwrap();
    assert!(into_a.changed() && into_a.formatting && into_a.edits.is_empty(), "{into_a:?}");
    let into_b = b.replica.merge_from(&a.replica).unwrap();
    assert_eq!(into_b.edits, [TextEdit::Insert { at: 5, len: 1 }]);
    let (da, db) = (a.replica.rebuild(&base).unwrap(), b.replica.rebuild(&base).unwrap());
    assert_eq!(da, db);
    assert!(da.paragraphs[0].runs[0].style.italic && sequence_text(&da).starts_with("Title!"));
    assert!(!a.replica.merge_from(&b.replica).unwrap().changed(), "nothing left to take");
}

#[test]
fn a_snapshot_that_is_not_a_letters_document_is_refused() {
    let other = LoroDoc::new();
    other.get_text("body").insert(0, "no paragraph style").unwrap();
    other.commit();
    let snapshot = other.export(loro::ExportMode::Snapshot).unwrap();
    let error = Replica::from_snapshot(1, &snapshot, &sample()).err().unwrap();
    assert!(matches!(error, CollabError::Invalid(_)), "{error:?}");
    assert!(matches!(Replica::from_snapshot(1, b"not a snapshot", &sample()).err(), Some(CollabError::Decode(_))));
}

#[test]
fn a_version_that_is_not_one_is_refused() {
    let r = Replica::new(1, &sample()).unwrap();
    assert!(matches!(r.export_since(b"not a version"), Err(CollabError::Decode(_))));
}

// ------------------------------------------------------------------ what is refused

fn assert_refused<T>(result: Result<T, CollabError>, what: Unsupported) {
    assert_eq!(result.err(), Some(CollabError::Unsupported(what)));
}

#[test]
fn tables_are_refused_with_an_error_not_encoded_as_text() {
    let mut d = Document::from_plain_text("a\nb");
    d.paragraphs[1].style.table_cell = Some(TableCell { table: 0, row: 0, col: 0 });
    assert_refused(Replica::new(1, &d), Unsupported::Tables);

    let d = Document::from_plain_text("a");
    let mut r = Replica::new(1, &d).unwrap();
    let before = (r.version_bytes(), r.paragraphs().unwrap());
    let cell = Paragraph { style: ParaStyle { table_cell: Some(TableCell { table: 0, row: 0, col: 0 }), ..Default::default() }, runs: vec![Run::plain("c")] };
    for op in [
        Op::SetParagraphs { para: 0, remove: 1, insert: vec![] },
        Op::Insert { at: 1, content: vec![Paragraph::default(), cell] },
        Op::SetParaStyle { at: 0, style: ParaStyle { table_cell: Some(TableCell { table: 0, row: 0, col: 0 }), ..Default::default() } },
    ] {
        assert_refused(r.record(&d, &op), Unsupported::Tables);
    }
    // And a document that has tables in it, edited.
    let mut with_table = Document::from_plain_text("x\ny");
    with_table.paragraphs[1].style.table_cell = Some(TableCell { table: 0, row: 0, col: 0 });
    assert_refused(r.record(&with_table, &Op::Delete { at: 0, len: 2 }), Unsupported::Tables);
    assert_refused(r.record(&with_table, &Op::Insert { at: 2, content: vec![Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("z")] }] }), Unsupported::Tables);
    assert_eq!((r.version_bytes(), r.paragraphs().unwrap()), before, "a refused op writes nothing");
}

#[test]
fn footnotes_are_refused_with_an_error() {
    let mut d = Document::from_plain_text("a");
    d.footnotes = vec!["a note".into()];
    assert_refused(Replica::new(1, &d), Unsupported::Footnotes);

    let d = Document::from_plain_text("a");
    let mut r = Replica::new(1, &d).unwrap();
    let before = r.version_bytes();
    let reference = Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } };
    assert_refused(r.record(&d, &Op::Insert { at: 1, content: vec![Paragraph { style: ParaStyle::default(), runs: vec![reference.clone()] }] }), Unsupported::Footnotes);
    assert_refused(r.record(&d, &Op::SetFootnotes { notes: vec!["a note".into()] }), Unsupported::Footnotes);
    let mut with_reference = Document::from_plain_text("a");
    with_reference.paragraphs[0].runs.push(reference);
    assert_refused(Replica::new(1, &with_reference), Unsupported::Footnotes);
    assert_eq!(r.version_bytes(), before);
}

#[test]
fn headers_and_footers_are_refused_with_an_error() {
    for set in [
        (|d: &mut Document| d.header = Some("Page {page}".into())) as fn(&mut Document),
        |d| d.footer = Some("Confidential".into()),
        |d| d.header_alignment = Alignment::Right,
        |d| d.first_header = Some(Default::default()),
        |d| d.header_lines = vec![Default::default()],
    ] {
        let mut d = Document::from_plain_text("a");
        set(&mut d);
        assert_refused(Replica::new(1, &d), Unsupported::HeadersFooters);
    }
    let d = Document::from_plain_text("a");
    let mut r = Replica::new(1, &d).unwrap();
    assert_refused(r.record(&d, &Op::SetHeaderFooter { header: Some("h".into()), footer: None }), Unsupported::HeadersFooters);
}

#[test]
fn sections_comments_and_unmarked_formatting_are_refused_with_an_error() {
    let mut d = Document::from_plain_text("a");
    d.sections = vec![DocSection { id: 0, range: ParagraphRange::new(0, 1), geometry: page(), header: None, footer: None }];
    assert_refused(Replica::new(1, &d), Unsupported::Sections);
    let d = Document::from_plain_text("a");
    let mut r = Replica::new(1, &d).unwrap();
    assert_refused(r.record(&d, &Op::SetSections { sections: vec![] }), Unsupported::Sections);

    let mut d = Document::from_plain_text("a");
    d.comments = vec![Comment { id: 0, author: "Ada".into(), date: "2026-10-26T10:00:00Z".into(), text: "hm".into(), resolved: false, parent: None }];
    assert_refused(Replica::new(1, &d), Unsupported::Comments);
    let d = Document::from_plain_text("abc");
    let mut r = Replica::new(1, &d).unwrap();
    assert_refused(r.record(&d, &Op::SetComment { id: 0, comment: None }), Unsupported::Comments);
    assert_refused(r.record(&d, &Op::Mark { start: 0, end: 2, key: MarkKey::Comment(0), value: RunStyle { comments: vec![0], ..Default::default() } }), Unsupported::Comments);
    let commented = Run { text: "x".into(), style: RunStyle { comments: vec![3], ..Default::default() } };
    assert_refused(r.record(&d, &Op::Insert { at: 0, content: vec![Paragraph { style: ParaStyle::default(), runs: vec![commented] }] }), Unsupported::Comments);

    // Formatting no mark carries: an image's extent on a text run.
    let stray = Run { text: "x".into(), style: RunStyle { image_extent_emu: Some((1, 2)), ..Default::default() } };
    assert_refused(r.record(&d, &Op::Insert { at: 0, content: vec![Paragraph { style: ParaStyle::default(), runs: vec![stray] }] }), Unsupported::RunStyle);
    let object = Run { text: "\u{FFFC}".into(), style: RunStyle::default() };
    assert!(matches!(
        r.record(&d, &Op::Insert { at: 0, content: vec![Paragraph { style: ParaStyle::default(), runs: vec![object] }] }),
        Err(CollabError::Invalid(_))
    ));
}

/// An update made by an editor that does not follow the encoding.
fn foreign(build: impl FnOnce(&LoroDoc)) -> Vec<u8> {
    let doc = LoroDoc::new();
    doc.set_peer_id(77).unwrap();
    // Loro refuses a mark whose key has no expand rule; this editor defines
    // the ones of the encoding, and a default for the key it invents.
    doc.config_text_style(style_config());
    doc.config_default_text_style(Some(StyleConfig { expand: ExpandType::After }));
    build(&doc);
    doc.commit();
    doc.export(loro::ExportMode::all_updates()).unwrap()
}

fn table_style() -> ParaStyle {
    ParaStyle { table_cell: Some(TableCell { table: 0, row: 0, col: 0 }), ..Default::default() }
}

/// An update that would make the document something the encoding refuses is
/// an error, and the live document is exactly as it was: not a different
/// version, not a different paragraph.
#[test]
fn a_refused_update_leaves_the_live_document_untouched() {
    let base = sample();
    let (mut a, mut b) = peers(&base);
    b.type_text(0, "b");
    let good = since(&b, &a);
    a.type_text(0, "a");
    let obj_with = |run: Run| move |d: &LoroDoc| {
        let t = d.get_text("body");
        t.insert(0, "\u{FFFC}").unwrap();
        t.mark(0..1, "obj", serde_json::to_string(&run).unwrap()).unwrap();
    };
    let footnote = Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } };
    let commented = Run { text: "pic".into(), style: RunStyle { image: Some("x.png".into()), comments: vec![1], ..Default::default() } };
    type Check = fn(&CollabError) -> bool;
    let cases: Vec<(&str, Vec<u8>, Check)> = vec![
        (
            "a table cell",
            foreign(|d| {
                let t = d.get_text("body");
                t.insert(0, "x\ny").unwrap();
                t.mark(1..2, "p", serde_json::to_string(&table_style()).unwrap()).unwrap();
            }),
            |e| *e == CollabError::Unsupported(Unsupported::Tables),
        ),
        ("a footnote reference", foreign(obj_with(footnote)), |e| *e == CollabError::Unsupported(Unsupported::Footnotes)),
        ("a comment on an object", foreign(obj_with(commented)), |e| *e == CollabError::Unsupported(Unsupported::Comments)),
        (
            "a flag that is text",
            foreign(|d| {
                let t = d.get_text("body");
                t.insert(0, "abc").unwrap();
                t.mark(0..2, "bold", "yes").unwrap();
            }),
            |e| matches!(e, CollabError::Invalid(_)),
        ),
        (
            "a size that is out of range",
            foreign(|d| {
                let t = d.get_text("body");
                t.insert(0, "abc").unwrap();
                t.mark(0..2, "size", 70_000i64).unwrap();
            }),
            |e| matches!(e, CollabError::Invalid(_)),
        ),
        (
            "a mark nobody defined",
            foreign(|d| {
                let t = d.get_text("body");
                t.insert(0, "abc").unwrap();
                t.mark(0..2, "sparkle", true).unwrap();
            }),
            |e| matches!(e, CollabError::Invalid(_)),
        ),
        (
            "an object without its run",
            foreign(|d| d.get_text("body").insert(0, "\u{FFFC}").unwrap()),
            |e| matches!(e, CollabError::Invalid(_)),
        ),
        (
            "a paragraph style that is not one",
            foreign(|d| {
                let t = d.get_text("body");
                t.insert(0, "a\nb").unwrap();
                t.mark(1..2, "p", "{\"heading\": \"high\"}").unwrap();
            }),
            |e| matches!(e, CollabError::Invalid(_)),
        ),
        ("bytes that are no update", b"not an update at all".to_vec(), |e| matches!(e, CollabError::Decode(_))),
        (
            "an update with a bit flipped",
            {
                let mut bytes = good.clone();
                let last = bytes.len() - 1;
                bytes[last] ^= 0x01;
                bytes
            },
            |e| matches!(e, CollabError::Decode(_)),
        ),
    ];
    for (what, update, expected) in cases {
        let before = (a.replica.version_bytes(), a.replica.paragraphs().unwrap(), a.replica.export_snapshot().unwrap());
        let error = a.replica.import_updates(&update).err().unwrap_or_else(|| panic!("{what}: was taken in"));
        assert!(expected(&error), "{what}: {error:?}");
        assert_eq!(a.replica.version_bytes(), before.0, "{what}: the version moved");
        assert_eq!(a.replica.paragraphs().unwrap(), before.1, "{what}: the document changed");
        assert_eq!(a.replica.export_snapshot().unwrap(), before.2, "{what}: the state changed");
        a.assert_in_step(what);
    }
    // The replica still works: a good update is taken in after the bad ones.
    let report = a.replica.import_updates(&good).unwrap();
    assert!(report.changed() && matches!(report.edits.as_slice(), [TextEdit::Insert { len: 1, .. }]), "{report:?}");
    // A foreign update that does follow the encoding is taken in.
    let fine = foreign(|d| {
        let t = d.get_text("body");
        t.insert(0, "Z").unwrap();
        t.mark(0..1, "bold", true).unwrap();
    });
    let report = a.replica.import_updates(&fine).unwrap();
    assert!(report.changed());
    // Where a text that was inserted into an empty document lands among the
    // others is Loro's (by peer id): look at every paragraph.
    let paragraphs = a.replica.paragraphs().unwrap();
    assert!(paragraphs.iter().flat_map(|p| &p.runs).any(|r| r.text.contains('Z') && r.style.bold), "{paragraphs:?}");
}

// ------------------------------------------------------------------ offsets

/// The model counts chars, and so do the edits a report carries: an emoji is
/// one.
#[test]
fn report_edits_count_chars_not_bytes() {
    let base = Document::from_plain_text("a😀b");
    let (mut a, mut b) = peers(&base);
    let to_a = b.edit(Op::Delete { at: 1, len: 1 });
    let report = a.receive(&to_a);
    assert_eq!(report.edits, [TextEdit::Delete { at: 1, len: 1 }]);
    assert_eq!(a.text(), "ab");
    let to_b = a.type_text(2, "é😀");
    let report = b.receive(&to_b);
    assert_eq!(report.edits, [TextEdit::Insert { at: 2, len: 2 }]);
    assert_eq!(b.text(), "abé😀");
}

// ------------------------------------------------------------------ undo (ADR 0011)

#[test]
fn undo_after_a_peer_typed_before_it_takes_back_only_my_text() {
    let base = Document::from_plain_text("hello");
    let (mut a, mut b) = peers(&base);
    let to_b = a.type_text(5, " world");
    let to_a = b.type_text(0, "Say: ");
    b.receive(&to_b);
    a.receive(&to_a);
    assert_eq!(a.text(), "Say: hello world");

    let undone = a.undo().expect("a step to undo");
    assert_eq!(a.text(), "Say: hello", "my words went, the peer's stayed");
    b.receive(&undone);
    assert_eq!(b.doc.paragraphs, a.doc.paragraphs, "an undo is an ordinary edit: the peer has it");

    // And redo, after the peer typed at the very place it will go: the
    // peer's text goes first at one offset.
    let to_a = b.type_text(b.text().chars().count(), "!");
    a.receive(&to_a);
    let redone = a.redo().expect("a step to redo");
    assert_eq!(a.text(), "Say: hello! world");
    b.receive(&redone);
    assert_eq!(b.doc.paragraphs, a.doc.paragraphs);
}

#[test]
fn a_peers_text_inside_my_typed_text_survives_my_undo() {
    let base = Document::from_plain_text("ab");
    let (mut a, mut b) = peers(&base);
    let to_b = a.type_text(1, "XYZ");
    b.receive(&to_b);
    let to_a = b.type_text(3, "!");
    a.receive(&to_a);
    assert_eq!(a.text(), "aXY!Zb");
    let undone = a.undo().unwrap();
    assert_eq!(a.text(), "a!b", "X, Y and Z went; the peer's '!' between them stayed");
    b.receive(&undone);
    assert_eq!(b.text(), "a!b");
}

#[test]
fn a_step_whose_text_a_peer_deleted_is_dropped_and_the_others_stay() {
    let base = Document::from_plain_text("start");
    let (mut a, mut b) = peers(&base);
    let one = a.type_text(5, " one");
    let two = a.type_text(9, " two");
    b.receive(&one);
    b.receive(&two);
    let to_a = b.edit(Op::Delete { at: 9, len: 4 });
    a.receive(&to_a);
    assert_eq!(a.text(), "start one");

    // The ' two' step has nothing left to take back: it went when the
    // peer's delete arrived, and the step before it is still there.
    let undone = a.undo().expect("the ' one' step is still there");
    assert_eq!(a.text(), "start");
    b.receive(&undone);
    assert_eq!(b.text(), "start");
    assert!(a.undo().is_none() && !a.history.can_undo(), "and that was all of them");
}

#[test]
fn a_restyle_of_a_paragraph_whose_break_a_peer_deleted_is_dropped() {
    let base = Document::from_plain_text("one\ntwo");
    let (mut a, mut b) = peers(&base);
    let heading = ParaStyle { heading: Some(1), ..Default::default() };
    a.edit(Op::SetParaStyle { at: 4, style: heading });
    let to_a = b.edit(Op::Delete { at: 3, len: 1 });
    a.receive(&to_a);
    assert_eq!(a.text(), "onetwo");
    assert!(!a.history.can_undo(), "the paragraph it restyled is gone, and its step with it");
    assert!(a.undo().is_none());
    assert_eq!(a.doc.paragraphs.len(), 1);
}

/// A dropped step leaves the document without its undo: the step below it is
/// moved over that as well as over the peer's edit.
#[test]
fn a_dropped_step_does_not_misplace_the_steps_below_it() {
    let base = Document::from_plain_text("p1\np2");
    let (mut a, mut b) = peers(&base);
    a.type_text(5, "X");
    a.edit(Op::SetParaStyle { at: 3, style: ParaStyle { heading: Some(1), ..Default::default() } });
    let to_a = b.edit(Op::Delete { at: 2, len: 1 });
    a.receive(&to_a);
    assert_eq!(a.text(), "p1p2X");
    // The restyle's step is gone; what is left is the typing step.
    let undone = a.undo().expect("the typing step");
    assert_eq!(a.text(), "p1p2", "it took back the X, not the char next to it");
    b.receive(&undone);
    assert_eq!(b.text(), "p1p2");
    assert!(!a.history.can_undo(), "and the restyle is not there to undo");
}

// ------------------------------------------------------------------ the transform itself

/// A small deterministic generator (the tests must not depend on a seed
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

/// Valid sequential edits from a text of `len` chars.
fn random_edits(rng: &mut Rng, mut len: usize) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    for _ in 0..rng.below(4) {
        if rng.below(2) == 0 || len == 0 {
            let (at, n) = (rng.below(len + 1), 1 + rng.below(3));
            edits.push(TextEdit::Insert { at, len: n });
            len += n;
        } else {
            let at = rng.below(len);
            let n = 1 + rng.below((len - at).min(4));
            edits.push(TextEdit::Delete { at, len: n });
            len -= n;
        }
    }
    edits
}

/// Apply `edits` to `text`, an insert writing the next of `letters`
/// repeated. `None` when an edit is out of range.
fn run(text: &mut Vec<char>, edits: &[TextEdit], letters: &[char]) -> Option<()> {
    let mut letters = letters.iter();
    for edit in edits {
        match *edit {
            TextEdit::Insert { at, len } => {
                if at > text.len() {
                    return None;
                }
                let letter = *letters.next()?;
                for _ in 0..len {
                    text.insert(at, letter);
                }
            }
            TextEdit::Delete { at, len } => {
                if at + len > text.len() {
                    return None;
                }
                drop(text.drain(at..at + len));
            }
        }
    }
    Some(())
}

/// The core of the rebase: two lists of edits made from one text, each
/// transformed over the other, give the same text whichever comes first.
#[test]
fn transformed_edits_converge() {
    let start: Vec<char> = "0123456789abcdef".chars().collect();
    let mut rng = Rng(0x7E57_AB1E);
    for case in 0..4000 {
        let a = random_edits(&mut rng, start.len());
        let b = random_edits(&mut rng, start.len());
        let (a2, b2) = it_lists(&a, &b);
        let (letters_a, letters_b) = (['A', 'B', 'C', 'D'], ['w', 'x', 'y', 'z']);
        let mut first = start.clone();
        let mut second = start.clone();
        let ok = run(&mut first, &a, &letters_a)
            .and_then(|()| run(&mut first, &b2, &letters_b))
            .and_then(|()| run(&mut second, &b, &letters_b))
            .and_then(|()| run(&mut second, &a2, &letters_a));
        assert!(ok.is_some(), "case {case}: {a:?} over {b:?} left a range outside the text: {a2:?}, {b2:?}");
        assert_eq!(first, second, "case {case}: {a:?} and {b:?} diverge");
    }
}

// ------------------------------------------------------------------ random editing

/// A random local edit on `d`: typing, Enter, a delete, bold, italic, a
/// link, or a paragraph style.
fn random_op(rng: &mut Rng, d: &Document) -> Op {
    let len = doc_len(d);
    let at = rng.below(len + 1);
    match rng.below(7) {
        0 | 1 => typing(d, at, ["a", "bc", " ", "xyz", "é😀"][rng.below(5)]).unwrap(),
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
            let (key, value) = if rng.below(2) == 0 { (MarkKey::Italic, italic()) } else { (MarkKey::Link, link()) };
            Op::Mark { start: at.min(end), end, key, value }
        }
        _ => {
            let (p, _) = edit::locate(d, at).unwrap();
            let style = ParaStyle { heading: [None, Some(1), Some(2)][rng.below(3)], ..d.paragraphs[p].style.clone() };
            Op::SetParaStyle { at, style }
        }
    }
}

/// One random action: an edit (most often), an undo or a redo.
fn act(rng: &mut Rng, peer: &mut Peer) {
    match rng.below(10) {
        0 | 1 => {
            peer.undo();
        }
        2 => {
            peer.redo();
        }
        _ => {
            let op = random_op(rng, &peer.doc);
            if apply(&mut peer.doc.clone(), &op).is_ok() {
                peer.edit(op);
            }
        }
    }
}

/// Two peers editing, undoing and redoing at random, taking each other's
/// updates now and then: each replica is its own peer's model between
/// exchanges, every undo or redo the history lets through applies to both the
/// model and the replica, and after every exchange both show the same
/// document.
#[test]
fn two_peers_editing_and_undoing_at_random_converge() {
    for seed in 1..=24u64 {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed);
        let base = sample();
        let (mut a, mut b) = peers(&base);
        for round in 0..20 {
            for _ in 0..rng.below(6) {
                act(&mut rng, &mut a);
            }
            for _ in 0..rng.below(6) {
                act(&mut rng, &mut b);
            }
            a.assert_in_step(&format!("seed {seed} round {round}: peer 1"));
            b.assert_in_step(&format!("seed {seed} round {round}: peer 2"));
            exchange(&mut a, &mut b);
            assert_eq!(a.doc.paragraphs, b.doc.paragraphs, "seed {seed} round {round}: the peers diverge");
            assert!(!a.doc.paragraphs.is_empty());
        }
        let (_, replay) = Replica::from_updates(9, &a.replica.export_all().unwrap(), &base).unwrap();
        assert_eq!(replay.paragraphs, a.doc.paragraphs, "seed {seed}: a replay differs");
    }
}
