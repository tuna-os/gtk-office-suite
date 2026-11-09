//! The composer's own undo history (#200), interleaved with the body's text
//! history in WebKit.

use super::messages::{ComposeInput, ComposeOutput};
use super::types::{ComposeStep, ComposeUndoEntry};
use super::Compose;
use super::ComposeWidgets;
use crate::i18n::i18n;
use crate::ui::rich_editor::RichEditor;
use relm4::prelude::*;

/// A marker for the body's own text history, named the way the user would
/// name what it takes back.
fn body_entry() -> ComposeUndoEntry {
    ComposeUndoEntry {
        step: ComposeStep::Body,
        what: i18n("Typing"),
    }
}

impl Compose {
    /// Record a change the composer can take back (#200), ending whatever
    /// redo branch was open — as every undo history does.
    ///
    /// A change of the composer's own is filed *under* the body's marker, so
    /// the marker always sits on top: what someone was just writing comes
    /// back first, and their attachments after it, newest first. The order
    /// has to be decided here because it cannot be honoured any finer —
    /// WebKit's text history can be stepped but not counted, so a marker has
    /// no way to say that two of its steps belong above an attachment and
    /// the rest below it. Writing wins, which is what a composer is for.
    pub(super) fn push_history(&mut self, what: String, step: ComposeStep) {
        let at = match self.undo_stack.last().map(|e| &e.step) {
            Some(ComposeStep::Body) => self.undo_stack.len() - 1,
            _ => self.undo_stack.len(),
        };
        self.undo_stack.insert(at, ComposeUndoEntry { step, what });
        self.redo_stack.clear();
    }

    /// Step the composer's history one change back, or forward with `redo`.
    ///
    /// A [`ComposeStep::Body`] marker is not popped when it is used: the
    /// editor holds an unknown number of text steps behind it, so the marker
    /// stays where it is and a matching one is put on the other side, until
    /// the editor says it has run out. Only then does it cross over and the
    /// change under it come up. That reading of the editor is a beat behind
    /// the command that changed it, which costs at worst one keypress that
    /// does nothing — never a step out of order.
    pub(super) fn history_step(
        &mut self,
        redo: bool,
        widgets: &ComposeWidgets,
        sender: &ComponentSender<Self>,
    ) {
        let (mut from, mut to) = if redo {
            (
                std::mem::take(&mut self.redo_stack),
                std::mem::take(&mut self.undo_stack),
            )
        } else {
            (
                std::mem::take(&mut self.undo_stack),
                std::mem::take(&mut self.redo_stack),
            )
        };
        let mut changed_attachments = false;
        while let Some(entry) = from.last() {
            if matches!(entry.step, ComposeStep::Body) {
                if if redo {
                    self.editor.can_redo()
                } else {
                    self.editor.can_undo()
                } {
                    if redo {
                        self.editor.redo();
                    } else {
                        self.editor.undo();
                    }
                    // Whatever the editor just took back is now available
                    // in the other direction, at this point in the order.
                    if !matches!(to.last().map(|e| &e.step), Some(ComposeStep::Body)) {
                        to.push(body_entry());
                    }
                    break;
                }
                // Run out (or a format change reloaded the document and
                // dropped its history): hand the marker over — dropping it
                // if that side already ends in one — and go on to the
                // change underneath.
                from.pop();
                if !matches!(to.last().map(|e| &e.step), Some(ComposeStep::Body)) {
                    to.push(body_entry());
                }
                continue;
            }
            let entry = from.pop().expect("checked by the loop condition");
            let back = self.apply_step(entry.step);
            to.push(ComposeUndoEntry {
                step: back,
                what: entry.what,
            });
            changed_attachments = true;
            break;
        }
        if redo {
            self.redo_stack = from;
            self.undo_stack = to;
        } else {
            self.undo_stack = from;
            self.redo_stack = to;
        }
        if changed_attachments {
            self.rebuild_attachments(&widgets.attach_box, sender);
        }
        self.report_history(sender);
    }

    /// Carry out one step, returning the step that puts it back.
    fn apply_step(&mut self, step: ComposeStep) -> ComposeStep {
        match step {
            // Never reached: the caller keeps body markers on their stack.
            ComposeStep::Body => ComposeStep::Body,
            ComposeStep::Insert { at, paths } => {
                let at = at.min(self.attachments.len());
                let count = paths.len();
                self.attachments.splice(at..at, paths);
                ComposeStep::Remove { at, count }
            }
            ComposeStep::Remove { at, count } => {
                let at = at.min(self.attachments.len());
                let end = (at + count).min(self.attachments.len());
                let paths: Vec<_> = self.attachments.drain(at..end).collect();
                ComposeStep::Insert { at, paths }
            }
        }
    }

    /// Tell the host what the history can do now, for its own Undo and Redo
    /// entries. The body's side is read from the editor rather than from the
    /// stack: a marker that has run out is still sitting there until the
    /// next step walks past it.
    pub(super) fn report_history(&self, sender: &ComponentSender<Self>) {
        let reachable = |stack: &[ComposeUndoEntry], can_text: bool| {
            stack
                .iter()
                .rev()
                .find(|e| !matches!(e.step, ComposeStep::Body) || can_text)
                .map(|e| e.what.clone())
        };
        let _ = sender.output(ComposeOutput::History {
            id: self.compose_id,
            undo: reachable(&self.undo_stack, self.editor.can_undo()),
            redo: reachable(&self.redo_stack, self.editor.can_redo()),
        });
    }

    /// Showcase only: walk the history through a round that mixes typing
    /// with attachments, logging the body and the attachment names after
    /// each step on `hylki::compose::undo`. The keys cannot be injected on
    /// a Wayland desktop, so this is how the interleaving is checked.
    ///
    /// Each step schedules the next a beat later, because both sides of it
    /// are asynchronous: the document's edits go to the web process, and
    /// the editor state that reports them comes back the same way.
    pub(super) fn showcase_history(
        &mut self,
        step: u8,
        widgets: &ComposeWidgets,
        sender: &ComponentSender<Self>,
    ) {
        let stack = |s: &[ComposeUndoEntry]| {
            s.iter()
                .map(|e| e.what.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let names = |p: &[std::path::PathBuf]| {
            p.iter()
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        // Source mode is a textarea rather than the rich document, and takes
        // the same route through WebKit's history — worth walking too, since
        // three of the four composing formats are written in it (#180).
        let focus_js = if self.editor.source_kind().is_some() {
            "var t=document.getElementById('src');t.focus();\
             t.selectionStart=t.selectionEnd=t.value.length;"
        } else {
            "document.body.focus();"
        };
        // HYLKI_SHOWCASE_COMPOSE_UNDO=backspace trims the body with the
        // Backspace key's own editing command instead of typing into it,
        // which is how a reply is usually cut down to size.
        let mode = std::env::var("HYLKI_SHOWCASE_COMPOSE_UNDO").unwrap_or_default();
        let deleting = mode == "backspace";
        // `pause` leaves more than the script's PAUSE_MS between two runs of
        // typing, so each should come back on a press of its own.
        let pausing = mode == "pause";
        let type_in = |editor: &RichEditor, text: &str| {
            editor.run_js(&format!(
                "{focus_js}document.execCommand('insertText',false,'{text}')"
            ));
        };
        let attach = |sender: &ComponentSender<Self>| {
            let path = std::env::temp_dir().join("hylki-undo-probe.txt");
            let _ = std::fs::write(&path, b"probe");
            sender
                .input_sender()
                .emit(ComposeInput::AddAttachments(vec![path]));
        };
        if step == 0 {
            // The widget has to hold focus, not just the document, or the
            // window's own Undo entry never changes hands.
            self.editor.grab_focus();
            self.editor.run_js(focus_js);
        }
        if deleting {
            // Something to delete first — a caret at the top of a fresh
            // reply has nothing behind it, and Backspace there is a no-op
            // that rightly leaves no step to take back.
            match step {
                0 => type_in(&self.editor, "ONETWO"),
                1..=3 => self.editor.editing_command("DeleteBackward"),
                4 => attach(sender),
                5..=9 => sender
                    .input_sender()
                    .emit(ComposeInput::History { redo: false }),
                10..=12 => sender
                    .input_sender()
                    .emit(ComposeInput::History { redo: true }),
                _ => {}
            }
        } else if pausing {
            match step {
                0 => type_in(&self.editor, "ONE"),
                1 => {} // the long wait below
                2 => type_in(&self.editor, "TWO"),
                3..=6 => sender
                    .input_sender()
                    .emit(ComposeInput::History { redo: false }),
                7..=9 => sender
                    .input_sender()
                    .emit(ComposeInput::History { redo: true }),
                _ => {}
            }
        } else {
            match step {
                0 => type_in(&self.editor, "ONE"),
                1 => attach(sender),
                2 => type_in(&self.editor, "TWO"),
                3..=8 => sender
                    .input_sender()
                    .emit(ComposeInput::History { redo: false }),
                9..=11 => sender
                    .input_sender()
                    .emit(ComposeInput::History { redo: true }),
                _ => {}
            }
        }
        if step > 13 {
            return;
        }
        let s = sender.clone();
        let editor = self.editor.clone();
        let gap = if pausing && step == 1 { 7000 } else { 700 };
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(gap), move || {
            let report = move |body: String| {
                tracing::info!(target: "hylki::compose::undo", "step {step}: body {body:?}");
                s.input_sender()
                    .emit(ComposeInput::ShowcaseHistory(step + 1));
            };
            if editor.source_kind().is_some() {
                editor.extract_source(report);
            } else {
                editor.extract_html(report);
            }
        });
        tracing::info!(
            target: "hylki::compose::undo",
            "step {step}: undo [{}] redo [{}] text({},{}) attachments [{}]",
            stack(&self.undo_stack),
            stack(&self.redo_stack),
            self.editor.can_undo(),
            self.editor.can_redo(),
            names(&self.attachments),
        );
        let _ = widgets;
    }

    /// Handles [`ComposeInput::BodyHistory`].
    pub(super) fn on_body_history(&mut self, can_undo: bool, sender: &ComponentSender<Self>) {
        // A fresh edit takes its place in the order — one marker
        // covers a whole run of typing, since the editor coalesces
        // its own steps and this only has to say where they sit.
        if can_undo
            && !matches!(
                self.undo_stack.last().map(|e| &e.step),
                Some(ComposeStep::Body)
            )
        {
            self.push_history(i18n("Typing"), ComposeStep::Body);
        }
        self.report_history(sender);
    }
}
