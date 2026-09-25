// SPDX-License-Identifier: GPL-3.0-or-later
//
// ops.rs — the one op/history shape for every app (ADR 0011).
//
// A document changes only through ops. Applying an op returns its exact
// inverse: the ops that, applied in order, give the document back exactly
// as it was. Undo and redo are then nothing but applying inverses, and a
// step recorded here is the same thing a collaboration layer replays
// (RFC-0001): the op is the unit, the history is a stack of inverses.
//
// This started as Letters' `edit::History` (ADR 0010 stage 3c-3) and keeps
// its behaviour:
// - one user action is one step, whatever number of ops it took
//   (`begin`/`end`, or a single `record`);
// - typed word characters coalesce into one step (the app marks them with
//   `set_merge(true)`; `Op::coalesce` says how two inverses combine), and a
//   step that is not marked — a space, Enter, a paste — breaks the run, so
//   undo takes back a word at a time;
// - a new step clears redo;
// - if a stored step no longer applies (the document was replaced under
//   it), the history is dropped rather than half an undo applied.
//
// `UndoManager` (undo.rs) is the older command-object shape. New code uses
// this one; the apps migrate their own crates onto it.

/// A change to a `Doc` that knows its exact inverse.
pub trait Op: Sized + Clone {
    type Doc;
    type Error;

    /// Apply to `doc`. On success, return the inverse: ops that, applied in
    /// order, restore `doc` exactly. On failure, `doc` must be unchanged.
    fn apply(&self, doc: &mut Self::Doc) -> Result<Vec<Self>, Self::Error>;

    /// Fold `next` into `self`, where both are the one-op inverses of
    /// consecutive typed steps (`self` the earlier). Return `false` to keep
    /// them separate steps; the default never merges.
    fn coalesce(&mut self, _next: &Self) -> bool {
        false
    }
}

/// Apply `ops` in order. On the first failure, undo what was applied (so
/// `doc` is unchanged) and return the error. Returns the inverse of the
/// whole group, in the order to apply it.
pub fn apply_all<O: Op>(doc: &mut O::Doc, ops: &[O]) -> Result<Vec<O>, O::Error> {
    let mut undo: Vec<Vec<O>> = Vec::new();
    for op in ops {
        match op.apply(doc) {
            Ok(inverse) => undo.push(inverse),
            Err(e) => {
                for inverse in undo.into_iter().rev() {
                    let _ = apply_all(doc, &inverse);
                }
                return Err(e);
            }
        }
    }
    Ok(undo.into_iter().rev().flatten().collect())
}

/// Undo and redo from each change's inverse ops. One entry is one user
/// action (everything between `begin` and `end`, or one `record`).
#[derive(Debug, Clone)]
pub struct History<O> {
    undo: Vec<Vec<O>>,
    redo: Vec<Vec<O>>,
    open: Option<Vec<Vec<O>>>,
    /// The next recorded step is one typed word character: it joins the
    /// step before it when that was one too.
    merge: bool,
    /// The top undo step is typed word characters (it may take more).
    word: bool,
}

impl<O> Default for History<O> {
    fn default() -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), open: None, merge: false, word: false }
    }
}

impl<O: Op> History<O> {
    /// Start grouping changes into one undo step.
    pub fn begin(&mut self) {
        if self.open.is_none() {
            self.open = Some(Vec::new());
        }
    }

    /// Close the current group.
    pub fn end(&mut self) {
        if let Some(group) = self.open.take() {
            self.push(group);
        }
    }

    fn push(&mut self, group: Vec<Vec<O>>) {
        let ops: Vec<O> = group.into_iter().rev().flatten().collect();
        let merge = std::mem::take(&mut self.merge);
        if ops.is_empty() {
            return;
        }
        let word = std::mem::replace(&mut self.word, merge);
        self.redo.clear();
        // Typing extends the previous typing step: the undo of "ab" is one
        // step that removes both characters.
        if merge && word {
            if let ([next], Some([prev])) = (ops.as_slice(), self.undo.last_mut().map(Vec::as_mut_slice)) {
                if prev.coalesce(next) {
                    return;
                }
            }
        }
        self.undo.push(ops);
    }

    /// Mark the next recorded step as one typed word character; any step
    /// not marked (a space, Enter, anything else) breaks the run.
    pub fn set_merge(&mut self, merge: bool) {
        self.merge = merge;
    }

    /// Record the inverse ops of one applied change.
    pub fn record(&mut self, inverse: Vec<O>) {
        match &mut self.open {
            Some(group) => group.push(inverse),
            None => self.push(vec![inverse]),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo the last step on `doc`. Returns the ops applied (for a view to
    /// follow), or `None` when there is nothing to undo.
    pub fn undo(&mut self, doc: &mut O::Doc) -> Option<Vec<O>> {
        self.step(doc, false)
    }

    /// Redo the last undone step on `doc`.
    pub fn redo(&mut self, doc: &mut O::Doc) -> Option<Vec<O>> {
        self.step(doc, true)
    }

    fn step(&mut self, doc: &mut O::Doc, redo: bool) -> Option<Vec<O>> {
        self.end();
        self.word = false;
        let ops = if redo { self.redo.pop()? } else { self.undo.pop()? };
        match apply_all(doc, &ops) {
            Ok(inverse) => {
                if redo { &mut self.undo } else { &mut self.redo }.push(inverse);
                Some(ops)
            }
            Err(_) => {
                // History no longer fits the document: drop it rather than
                // apply half an undo.
                self.clear();
                None
            }
        }
    }

    /// Forget everything (a document was replaced wholesale).
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests;
