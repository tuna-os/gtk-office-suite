use super::super::rebase::it_lists;
use super::*;
use crate::edit::{apply, doc_len, typing, History, MarkKey};
use crate::model::RunStyle;

/// A small deterministic generator, so that a failing case is reproducible.
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

// ------------------------------------------------------------------ the mapping against the transform

fn random_edits(rng: &mut Rng, mut len: usize) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    for _ in 0..rng.below(5) {
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

/// An offset moves exactly as an insert of the user's at that offset does
/// when the undo history is rebased over the same edits (`Rebaser`): the
/// caret and the stored undo steps can never disagree about where text is.
#[test]
fn the_caret_moves_as_the_undo_steps_do() {
    let mut rng = Rng(0xCA2E_7001);
    for case in 0..4000 {
        let len = 12 + rng.below(8);
        let edits = random_edits(&mut rng, len);
        let at = rng.below(len + 1);
        let (mine, _) = it_lists(&[TextEdit::Insert { at, len: 1 }], &edits);
        assert_eq!(mine.len(), 1, "case {case}");
        let want = match mine[0] {
            TextEdit::Insert { at, .. } => at,
            other => panic!("case {case}: {other:?}"),
        };
        assert_eq!(map_offset(at, &edits, Bias::After), want, "case {case}: {at} over {edits:?}");
    }
}

#[test]
fn a_peers_insert_at_the_caret_goes_before_it_and_one_at_a_selections_edges_stays_outside() {
    let insert = [TextEdit::Insert { at: 5, len: 3 }];
    assert_eq!(map_offset(5, &insert, Bias::After), 8);
    assert_eq!(map_offset(5, &insert, Bias::Before), 5);
    assert_eq!(map_offset(4, &insert, Bias::After), 4);
    assert_eq!(map_offset(6, &insert, Bias::Before), 9);
    // The caret.
    assert_eq!(map_selection(5, 5, &insert), (8, 8));
    // A selection 5..9: text typed at 5 is not in it, and neither is text typed at 9.
    assert_eq!(map_selection(9, 5, &insert), (12, 8));
    assert_eq!(map_selection(5, 9, &[TextEdit::Insert { at: 9, len: 2 }]), (5, 9));
    // The direction is kept.
    assert_eq!(map_selection(9, 5, &[TextEdit::Insert { at: 9, len: 2 }]), (9, 5));
}

#[test]
fn text_a_peer_deleted_around_the_caret_leaves_it_where_the_text_was() {
    let delete = [TextEdit::Delete { at: 4, len: 6 }];
    assert_eq!(map_offset(3, &delete, Bias::After), 3);
    assert_eq!(map_offset(4, &delete, Bias::After), 4);
    assert_eq!(map_offset(7, &delete, Bias::After), 4);
    assert_eq!(map_offset(10, &delete, Bias::After), 4);
    assert_eq!(map_offset(11, &delete, Bias::After), 5);
    assert_eq!(map_selection(2, 12, &delete), (2, 6));
    assert_eq!(map_selection(5, 8, &delete), (4, 4));
}

// ------------------------------------------------------------------ real replicas

const BASE_LEN: usize = 40;

/// Forty distinct characters, which no edit of these tests types, so that
/// where one of them is, after the edits of a peer, says where a caret that
/// was next to it should be.
fn base_chars() -> Vec<char> {
    (0..BASE_LEN as u32).map(|i| char::from_u32(0x4E00 + i).unwrap()).collect()
}

fn base_doc() -> Document {
    Document::from_plain_text(&base_chars().iter().collect::<String>())
}

struct Side {
    shared: Shared,
    doc: Document,
    history: History,
}

impl Side {
    /// A local action: recorded, applied, put on the history.
    fn act(&mut self, ops: &[Op]) {
        self.shared.record(&self.doc, ops).expect("recorded");
        let mut inverse = Vec::new();
        for op in ops {
            inverse.push(apply(&mut self.doc, op).expect("applies"));
        }
        self.history.record(inverse.into_iter().rev().flatten().collect());
    }

    fn type_at(&mut self, at: usize, text: &str) {
        let op = typing(&self.doc, at, text).expect("typing");
        self.act(&[op]);
    }

    fn text(&self) -> String {
        sequence_text(&self.doc)
    }

    /// What the editor does with a peer's update: import, rebase the undo
    /// history, show the document.
    fn take(&mut self, update: &[u8]) -> Remote {
        let remote = self.shared.import(update, &self.doc).expect("imported");
        remote.report.rebase(&mut self.history);
        if let Some(document) = &remote.document {
            self.doc = document.clone();
        }
        remote
    }

    /// Undo or redo one step, as the editor does: on the model, then
    /// recorded as ordinary local ops. The update that carries it.
    fn walk(&mut self, redo: bool) -> Option<Vec<u8>> {
        let mut replay = self.doc.clone();
        let ops = (if redo { self.history.redo(&mut self.doc) } else { self.history.undo(&mut self.doc) })?;
        self.shared.record(&replay, &ops).expect("an undo is an ordinary action");
        for op in &ops {
            apply(&mut replay, op).expect("and applies");
        }
        assert_eq!(replay, self.doc);
        self.shared.outgoing().unwrap()
    }
}

fn pair() -> (Side, Side) {
    let base = base_doc();
    let (a, genesis) = Shared::start(1, &base).unwrap();
    let (b, b_doc) = Shared::join(2, &genesis, &base).unwrap();
    assert_eq!(b_doc.paragraphs, base.paragraphs);
    (Side { shared: a, doc: base, history: History::default() }, Side { shared: b, doc: b_doc, history: History::default() })
}

/// One random local action of `side`.
fn random_action(rng: &mut Rng, side: &mut Side) {
    let len = doc_len(&side.doc);
    let at = rng.below(len + 1);
    match rng.below(6) {
        0..=2 => side.type_at(at, ["a", "bc", " ", "xyz", "é😀", "\n"][rng.below(6)]),
        3 if len > 0 => {
            let at = rng.below(len);
            side.act(&[Op::Delete { at, len: (1 + rng.below(4)).min(len - at) }]);
        }
        4 => {
            let end = (at + 1 + rng.below(6)).min(len);
            if end > at {
                side.act(&[Op::Mark { start: at, end, key: MarkKey::Bold, value: RunStyle { bold: true, ..Default::default() } }]);
            }
        }
        _ => side.type_at(at, "\n"),
    }
}

/// Where the base characters of `base` still are in `text`, around the
/// gap `c` (between `base[c - 1]` and `base[c]`): the offset after the last
/// survivor to its left, the offset of the first survivor at or to its
/// right, and the offset of `base[c]` itself when it survived.
fn around(text: &[char], base: &[char], c: usize) -> (usize, usize, Option<usize>) {
    let pos = |ch: char| text.iter().position(|&t| t == ch);
    let left = base[..c].iter().rev().find_map(|&ch| pos(ch)).map_or(0, |i| i + 1);
    let right = base[c..].iter().find_map(|&ch| pos(ch)).unwrap_or(text.len());
    let exact = base.get(c).and_then(|&ch| pos(ch));
    (left, right, exact)
}

/// B edits while A's caret and selection sit in the text; A takes each
/// update in turn, mapping both. The caret stays between the characters it
/// was between, and a selection keeps the characters at its edges, whatever
/// B did around them, in every order and grouping of B's updates.
#[test]
fn a_peers_edits_never_move_the_caret_off_its_characters() {
    let base = base_chars();
    let (mut moved_carets, mut cases) = (0usize, 0usize);
    for seed in 1..=300u64 {
        let mut rng = Rng(0x5EED_0000 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let (mut a, mut b) = pair();
        let caret = rng.below(BASE_LEN + 1);
        let (s, e) = {
            let x = rng.below(BASE_LEN);
            (x, x + 1 + rng.below(BASE_LEN - x))
        };
        let (mut at, mut sel) = (caret, (s, e));
        let mut updates = Vec::new();
        for _ in 0..1 + rng.below(6) {
            random_action(&mut rng, &mut b);
            // Sometimes one update per action, sometimes several in one.
            if rng.below(2) == 0 {
                updates.extend(b.shared.outgoing().unwrap());
            }
        }
        updates.extend(b.shared.outgoing().unwrap());
        for update in &updates {
            let remote = a.take(update);
            at = map_offset(at, &remote.report.edits, Bias::After);
            sel = map_selection(sel.0, sel.1, &remote.report.edits);
        }
        assert_eq!(a.doc.paragraphs, b.doc.paragraphs, "seed {seed}: the peers diverge");
        let text: Vec<char> = a.text().chars().collect();
        let (left, right, exact) = around(&text, &base, caret);
        assert!(left <= at && at <= right, "seed {seed}: the caret {caret} is at {at}, outside {left}..={right} in {:?}", a.text());
        if let Some(exact) = exact {
            assert_eq!(at, exact, "seed {seed}: the caret in front of {:?} is not in front of it any more", base[caret]);
        }
        let (start, end) = (sel.0.min(sel.1), sel.0.max(sel.1));
        if let Some(i) = text.iter().position(|&t| t == base[s]) {
            assert_eq!(start, i, "seed {seed}: the selection {s}..{e} lost its first character");
        }
        if let Some(i) = text.iter().position(|&t| t == base[e - 1]) {
            assert_eq!(end, i + 1, "seed {seed}: the selection {s}..{e} lost its last character");
        }
        assert!(sel.0 <= text.len() && sel.1 <= text.len());
        moved_carets += usize::from(at != caret);
        cases += 1;
    }
    assert!(moved_carets * 4 > cases, "the test did not exercise the mapping: {moved_carets} of {cases} carets moved");
}

/// What goes wrong without the mapping, measured on the same cases as the
/// test above (`--success-output final` prints it): a caret left at its
/// offset, and a caret put at the end of the last edit (what the live model
/// does for the user's own edits).
#[test]
fn collab_offsets_how_often_the_caret_is_wrong_without_the_mapping() {
    let base = base_chars();
    let (mut unmoved_wrong, mut end_of_edit_wrong, mut mapped_wrong, mut cases) = (0usize, 0usize, 0usize, 0usize);
    for seed in 1..=300u64 {
        let mut rng = Rng(0x5EED_0000 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let (mut a, mut b) = pair();
        let caret = rng.below(BASE_LEN);
        let (mut mapped, mut end_of_edit) = (caret, caret);
        for _ in 0..1 + rng.below(4) {
            random_action(&mut rng, &mut b);
            let update = b.shared.outgoing().unwrap().expect("an update");
            let remote = a.take(&update);
            mapped = map_offset(mapped, &remote.report.edits, Bias::After);
            match remote.report.edits.last() {
                Some(TextEdit::Insert { at, len }) => end_of_edit = at + len,
                Some(TextEdit::Delete { at, .. }) => end_of_edit = *at,
                None => {}
            }
        }
        let text: Vec<char> = a.text().chars().collect();
        // Where the character that was right of the caret is now.
        let Some(want) = text.iter().position(|&t| t == base[caret]) else { continue };
        cases += 1;
        unmoved_wrong += usize::from(caret != want);
        end_of_edit_wrong += usize::from(end_of_edit != want);
        mapped_wrong += usize::from(mapped != want);
    }
    println!(
        "collab-offsets: over {cases} peer-edit sequences the caret was on the wrong character: left at its offset {unmoved_wrong}, at the end of the last edit {end_of_edit_wrong}, mapped {mapped_wrong}"
    );
    assert_eq!(mapped_wrong, 0);
    assert!(unmoved_wrong > 0 && end_of_edit_wrong > 0, "the naive placements were never wrong: the cases do not exercise the mapping");
}

// ------------------------------------------------------------------ the session's bytes

#[test]
fn collab_a_remote_update_is_never_sent_back() {
    let (mut a, mut b) = pair();
    a.type_at(3, "hello");
    let to_b = a.shared.outgoing().unwrap().expect("a delta");
    assert!(a.shared.outgoing().unwrap().is_none(), "taking a delta counts it as sent");
    let remote = b.take(&to_b);
    assert!(remote.report.changed() && remote.unsent.is_none());
    assert!(b.shared.outgoing().unwrap().is_none(), "what came from a peer is not B's to send");
    // The same update again changes nothing and still sends nothing.
    let again = b.take(&to_b);
    assert!(!again.report.changed() && again.document.is_none());
    assert!(b.shared.outgoing().unwrap().is_none());
    assert_eq!(a.doc.paragraphs, b.doc.paragraphs);
}

#[test]
fn collab_what_was_not_sent_when_an_update_arrives_is_handed_back_not_lost() {
    let (mut a, mut b) = pair();
    b.type_at(0, "B:");
    let to_a = b.shared.outgoing().unwrap().unwrap();
    // A typed too and has not sent it when B's update arrives.
    a.type_at(BASE_LEN, "!");
    let remote = a.take(&to_a);
    let unsent = remote.unsent.expect("A's own change is handed back");
    assert!(a.shared.outgoing().unwrap().is_none());
    let remote = b.take(&unsent);
    assert!(remote.report.changed());
    assert_eq!(a.doc.paragraphs, b.doc.paragraphs);
    assert!(a.text().starts_with("B:") && a.text().ends_with('!'));
}

/// Undo stays mine: A types, B types in the middle of A's word and elsewhere, A
/// undoes. A's typing goes, B's stays, on both, and A's undo is one update.
#[test]
fn collab_undo_after_a_peer_typed_takes_back_only_my_text() {
    let (mut a, mut b) = pair();
    a.type_at(10, "mine");
    let to_b = a.shared.outgoing().unwrap().unwrap();
    b.take(&to_b);
    b.type_at(12, "THEIRS");
    b.type_at(0, ">>");
    let to_a = b.shared.outgoing().unwrap().unwrap();
    a.take(&to_a);
    assert_eq!(a.text(), b.text());
    let undone = a.walk(false).expect("A has a step to undo");
    b.take(&undone);
    assert_eq!(a.doc.paragraphs, b.doc.paragraphs);
    let text = a.text();
    assert!(text.contains("THEIRS") && text.starts_with(">>"), "{text:?}");
    assert!(!text.contains("mine") && !text.contains('m') && !text.contains("ine"), "{text:?}");
    // And redo brings back exactly A's text.
    let redone = a.walk(true).expect("a step to redo");
    b.take(&redone);
    assert_eq!(a.doc.paragraphs, b.doc.paragraphs);
    assert!(a.text().contains("THEIRS") && a.text().contains('m'));
}

#[test]
fn collab_a_group_with_one_refused_op_writes_nothing() {
    let (mut a, _b) = pair();
    let before = a.shared.replica().version_bytes();
    let insert = typing(&a.doc, 3, "ok").unwrap();
    let group = [insert, Op::SetFootnotes { notes: vec!["a note".into()] }];
    let refused = a.shared.record(&a.doc, &group).unwrap_err();
    assert_eq!(refused, CollabError::Unsupported(super::super::Unsupported::Footnotes));
    assert!(a.shared.replica().is_at(&before).unwrap(), "the first op of the group was written before the second was refused");
    assert!(a.shared.outgoing().unwrap().is_none());
    assert_eq!(a.shared.replica().paragraphs().unwrap(), a.doc.paragraphs);
}

#[test]
fn collab_a_table_in_the_document_is_refused_before_a_session_starts() {
    let mut doc = base_doc();
    doc.paragraphs[0].style.table_cell = Some(crate::model::TableCell { table: 1, row: 0, col: 0 });
    let refused = Shared::start(1, &doc).err().expect("refused");
    assert_eq!(refused, CollabError::Unsupported(super::super::Unsupported::Tables));
}

#[test]
fn collab_a_join_whose_text_is_not_the_files_is_caught() {
    let base = base_doc();
    let (shared, genesis) = Shared::start(1, &base).unwrap();
    let (_joined, document) = Shared::join(2, &genesis, &base).unwrap();
    assert!(same_text(&document, &base).is_ok());
    let mut other = base.clone();
    other.paragraphs[0].runs = vec![crate::model::Run::plain("something else")];
    let why = same_text(&document, &other).unwrap_err();
    assert!(why.contains("first differ at character 0"), "{why}");
    drop(shared);
}
