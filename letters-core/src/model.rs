// model.rs — the document model: paragraphs of styled runs.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Invariants (enforced by normalize(), asserted by tests/model.rs):
//   1. Run.text is never empty.
//   2. Adjacent runs with identical style are merged.
//   3. A document always has at least one paragraph.
//
// Offsets are global character offsets; each paragraph break counts as one
// character, matching GtkTextBuffer's offset model exactly.

/// Character-level formatting. `Default` is unstyled text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub highlight: bool,
    /// Inline code (monospace) span.
    pub code: bool,
    pub link: Option<String>,
    /// Inline image: source path/URI. The run's text is the alt text.
    pub image: Option<String>,
    /// Font size in half-points (24 = 12pt); None = document default.
    pub font_size_hp: Option<u16>,
    /// Text color as RRGGBB hex (no '#'); None = default.
    pub color: Option<String>,
    /// Superscript/subscript position.
    pub vert_align: Option<VertAlign>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertAlign {
    Superscript,
    Subscript,
}

/// A partial style change: `None` fields are left untouched.
#[derive(Clone, Debug, Default)]
pub struct StylePatch {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub highlight: Option<bool>,
    pub code: Option<bool>,
    pub link: Option<Option<String>>,
}

impl StylePatch {
    pub fn set_bold(v: bool) -> Self { Self { bold: Some(v), ..Default::default() } }
    pub fn set_italic(v: bool) -> Self { Self { italic: Some(v), ..Default::default() } }
    pub fn set_underline(v: bool) -> Self { Self { underline: Some(v), ..Default::default() } }
    pub fn set_strikethrough(v: bool) -> Self { Self { strikethrough: Some(v), ..Default::default() } }
    pub fn set_highlight(v: bool) -> Self { Self { highlight: Some(v), ..Default::default() } }
    pub fn set_code(v: bool) -> Self { Self { code: Some(v), ..Default::default() } }
    pub fn set_link(v: Option<String>) -> Self { Self { link: Some(v), ..Default::default() } }

    pub fn apply_to(&self, style: &mut RunStyle) {
        if let Some(v) = self.bold { style.bold = v; }
        if let Some(v) = self.italic { style.italic = v; }
        if let Some(v) = self.underline { style.underline = v; }
        if let Some(v) = self.strikethrough { style.strikethrough = v; }
        if let Some(v) = self.highlight { style.highlight = v; }
        if let Some(v) = self.code { style.code = v; }
        if let Some(v) = &self.link { style.link = v.clone(); }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListKind {
    #[default]
    None,
    Bullet,
    Numbered,
}

/// Paragraph-level formatting.
#[derive(Clone, Debug, PartialEq)]
pub struct ParaStyle {
    /// Heading level 1..=6, or None for body text.
    pub heading: Option<u8>,
    pub alignment: Alignment,
    pub list: ListKind,
    /// Line spacing multiplier (1.0, 1.15, 1.5, 2.0).
    pub line_spacing: f32,
    /// Code-block membership: Some(language) marks this paragraph as a line
    /// of a code block ("" = no language). Consecutive code paragraphs with
    /// the same language form one block; paragraphs never contain newlines,
    /// so a multi-line block is a run of these.
    pub code_block: Option<String>,
    /// Block quote membership.
    pub block_quote: bool,
}

impl Default for ParaStyle {
    fn default() -> Self {
        Self { heading: None, alignment: Alignment::Left, list: ListKind::None, line_spacing: 1.0, code_block: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), style: RunStyle::default() }
    }
    pub fn char_len(&self) -> usize {
        self.text.chars().count()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paragraph {
    pub style: ParaStyle,
    pub runs: Vec<Run>,
}

impl Paragraph {
    pub fn char_len(&self) -> usize {
        self.runs.iter().map(Run::char_len).sum()
    }

    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Merge adjacent runs with equal style and drop empty runs.
    /// Image runs are kept even with empty alt text and never merged.
    fn normalize(&mut self) {
        self.runs.retain(|r| !r.text.is_empty() || r.style.image.is_some());
        let mut i = 0;
        while i + 1 < self.runs.len() {
            if self.runs[i].style == self.runs[i + 1].style {
                let next = self.runs.remove(i + 1);
                self.runs[i].text.push_str(&next.text);
            } else {
                i += 1;
            }
        }
    }

    /// Split at a character offset within this paragraph; self keeps the
    /// head, returns the tail (same paragraph style).
    fn split_at(&mut self, offset: usize) -> Paragraph {
        let mut head: Vec<Run> = Vec::new();
        let mut tail: Vec<Run> = Vec::new();
        let mut pos = 0;
        for run in self.runs.drain(..) {
            let len = run.char_len();
            if pos + len <= offset {
                head.push(run);
            } else if pos >= offset {
                tail.push(run);
            } else {
                let split = offset - pos;
                let byte = run.text.char_indices().nth(split).map(|(b, _)| b).unwrap_or(run.text.len());
                let (a, b) = run.text.split_at(byte);
                head.push(Run { text: a.to_string(), style: run.style.clone() });
                tail.push(Run { text: b.to_string(), style: run.style });
            }
            pos += len;
        }
        self.runs = head;
        self.normalize();
        let mut tail_para = Paragraph { style: self.style.clone(), runs: tail };
        tail_para.normalize();
        tail_para
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub paragraphs: Vec<Paragraph>,
}

impl Default for Document {
    fn default() -> Self { Self::new() }
}

impl Document {
    pub fn new() -> Self {
        Self { paragraphs: vec![Paragraph::default()] }
    }

    pub fn from_plain_text(text: &str) -> Self {
        let paragraphs = text
            .split('\n')
            .map(|line| Paragraph {
                style: ParaStyle::default(),
                runs: if line.is_empty() { vec![] } else { vec![Run::plain(line)] },
            })
            .collect::<Vec<_>>();
        let mut d = Self { paragraphs };
        d.ensure_non_empty();
        d
    }

    pub fn to_plain_text(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| p.text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Total character count; paragraph breaks count as one char each.
    pub fn char_len(&self) -> usize {
        let text: usize = self.paragraphs.iter().map(|p| p.char_len()).sum();
        text + self.paragraphs.len().saturating_sub(1)
    }

    fn ensure_non_empty(&mut self) {
        if self.paragraphs.is_empty() {
            self.paragraphs.push(Paragraph::default());
        }
    }

    /// Locate a global offset as (paragraph index, offset within paragraph).
    /// Offsets equal to a paragraph's length address its end (before the
    /// break); offset len+1 is the start of the next paragraph.
    fn locate(&self, offset: usize) -> (usize, usize) {
        let mut remaining = offset;
        for (i, p) in self.paragraphs.iter().enumerate() {
            let len = p.char_len();
            if remaining <= len {
                return (i, remaining);
            }
            remaining -= len + 1; // +1 for the paragraph break
        }
        let last = self.paragraphs.len() - 1;
        (last, self.paragraphs[last].char_len())
    }

    /// Style of the character at `offset` (or, at paragraph end / document
    /// end, the style a character typed there would inherit).
    pub fn style_at(&self, offset: usize) -> RunStyle {
        let (pi, po) = self.locate(offset);
        let para = &self.paragraphs[pi];
        let mut pos = 0;
        for run in &para.runs {
            let len = run.char_len();
            if po < pos + len {
                return run.style.clone();
            }
            pos += len;
        }
        // Past the last character: inherit from the paragraph's last run so
        // typing at the end continues the current style.
        para.runs.last().map(|r| r.style.clone()).unwrap_or_default()
    }

    pub fn insert_text(&mut self, offset: usize, text: &str) {
        if text.is_empty() { return; }
        let inherited = self.style_at(offset);
        let (pi, po) = self.locate(offset);

        let tail = self.paragraphs[pi].split_at(po);
        let mut lines = text.split('\n');

        // First line goes at the end of the head paragraph.
        if let Some(first) = lines.next() {
            if !first.is_empty() {
                self.paragraphs[pi].runs.push(Run { text: first.to_string(), style: inherited.clone() });
                self.paragraphs[pi].normalize();
            }
        }
        // Each further line starts a new paragraph (same para style).
        let mut insert_at = pi;
        for line in lines {
            insert_at += 1;
            let runs = if line.is_empty() {
                vec![]
            } else {
                vec![Run { text: line.to_string(), style: inherited.clone() }]
            };
            self.paragraphs.insert(insert_at, Paragraph { style: self.paragraphs[pi].style.clone(), runs });
        }
        // Re-attach the tail to the last inserted/updated paragraph.
        let last = &mut self.paragraphs[insert_at];
        last.runs.extend(tail.runs);
        last.normalize();
    }

    pub fn delete_range(&mut self, start: usize, end: usize) {
        if end <= start { return; }
        let (spi, spo) = self.locate(start);
        let (epi, epo) = self.locate(end);

        if spi == epi {
            let tail = self.paragraphs[spi].split_at(epo);
            self.paragraphs[spi].split_at(spo); // discard middle
            self.paragraphs[spi].runs.extend(tail.runs);
            self.paragraphs[spi].normalize();
        } else {
            let tail = self.paragraphs[epi].split_at(epo);
            self.paragraphs[spi].split_at(spo); // discard rest of start para
            self.paragraphs[spi].runs.extend(tail.runs);
            self.paragraphs[spi].normalize();
            self.paragraphs.drain(spi + 1..=epi);
        }
        self.ensure_non_empty();
    }

    /// Apply a style patch to every character in [start, end).
    pub fn apply_run_style(&mut self, start: usize, end: usize, patch: &StylePatch) {
        if end <= start { return; }
        let (spi, spo) = self.locate(start);
        let (epi, epo) = self.locate(end);

        for pi in spi..=epi {
            let para_len = self.paragraphs[pi].char_len();
            let from = if pi == spi { spo } else { 0 };
            let to = if pi == epi { epo } else { para_len };
            if from >= to { continue; }

            let tail = self.paragraphs[pi].split_at(to);
            let mut mid = self.paragraphs[pi].split_at(from);
            for run in &mut mid.runs {
                patch.apply_to(&mut run.style);
            }
            self.paragraphs[pi].runs.extend(mid.runs);
            self.paragraphs[pi].runs.extend(tail.runs);
            self.paragraphs[pi].normalize();
        }
    }

    /// Set or clear the heading level of one paragraph.
    pub fn set_heading(&mut self, para_idx: usize, level: Option<u8>) {
        if let Some(p) = self.paragraphs.get_mut(para_idx) {
            p.style.heading = level;
        }
    }
}
