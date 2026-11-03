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
// - if a stored step no longer applies (an edit that was not this history's
//   own changed what the step addresses: a peer's, in a collaboration),
//   that step alone is dropped, whole, and the others stay: never half an
//   undo applied, never the rest of the history lost with it.
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
    /// follow), or `None` when there is nothing to undo, or when the step
    /// no longer fits `doc` and was dropped (`doc` is unchanged then, and
    /// the next undo takes the step before it).
    pub fn undo(&mut self, doc: &mut O::Doc) -> Option<Vec<O>> {
        self.step(doc, false)
    }

    /// Redo the last undone step on `doc`. A step that no longer fits is
    /// dropped, as for [`History::undo`].
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
            // `apply_all` rolled the step back, so `doc` is as it was: no
            // half step. The step was popped and is not pushed anywhere:
            // this step is dropped and every other one is kept.
            Err(_) => None,
        }
    }

    /// Rewrite the stored steps after the document changed under them: a
    /// peer's edit arrived (ADR 0011, "Stored inverses are rebased on
    /// remote events"). `step` gets one step's ops and returns them as they
    /// apply to the changed document, or `None` to drop that step, that
    /// step alone; the others stay.
    ///
    /// Steps are visited in the order undo, and then redo, would apply
    /// them: the top one first, because a step lower down addresses the
    /// document as it is once the ones above it are undone. The group still
    /// open (if any) comes before the undo steps. `start` makes the state
    /// that `step` carries from one step to the next along a chain, once for
    /// the undo chain and once for the redo chain.
    pub fn rebase<S>(&mut self, start: impl Fn() -> S, mut step: impl FnMut(&mut S, &[O]) -> Option<Vec<O>>) {
        // A remote edit between two typed characters ends the typing run.
        self.word = false;
        let mut state = start();
        if let Some(group) = self.open.take() {
            let ops: Vec<O> = group.into_iter().rev().flatten().collect();
            // One entry, so the group's step, once closed, is these ops.
            self.open = Some(match step(&mut state, &ops) {
                Some(rebased) if !rebased.is_empty() => vec![rebased],
                _ => Vec::new(),
            });
        }
        self.undo = rebase_chain(std::mem::take(&mut self.undo), &mut state, &mut step);
        let mut state = start();
        self.redo = rebase_chain(std::mem::take(&mut self.redo), &mut state, &mut step);
    }

    /// Forget everything (a document was replaced wholesale).
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Rebase `steps` (oldest first) through `step`, top first; the ones it
/// drops, and the ones it leaves empty, are gone.
fn rebase_chain<O, S>(steps: Vec<Vec<O>>, state: &mut S, step: &mut impl FnMut(&mut S, &[O]) -> Option<Vec<O>>) -> Vec<Vec<O>> {
    let mut kept: Vec<Vec<O>> = steps.into_iter().rev().filter_map(|ops| step(state, &ops)).filter(|ops| !ops.is_empty()).collect();
    kept.reverse();
    kept
}

#[cfg(test)]
mod tests;
