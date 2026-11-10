// SPDX-License-Identifier: GPL-3.0-or-later
//! Converting between Letters' text and the suite's neutral text (ADR 0012,
//! Tier 3).
//!
//! `suite_common_core::document` holds the formatting every app of the suite
//! can show. Letters' [`RunStyle`] and [`ParaStyle`] hold that and more:
//! footnotes, chips, tracked changes, comments, images and text boxes,
//! indents, tab stops, borders and page flow. Going to the neutral form
//! drops the Letters-only part; coming back gives it its defaults. Nothing
//! is guessed in either direction.

use suite_common_core::document as neutral;

use crate::model::{ParaStyle, Paragraph, Run, RunStyle};

impl From<&RunStyle> for neutral::RunStyle {
    fn from(s: &RunStyle) -> Self {
        neutral::RunStyle {
            bold: s.bold,
            regular: s.regular,
            italic: s.italic,
            upright: s.upright,
            caps: s.caps,
            underline: s.underline,
            strikethrough: s.strikethrough,
            highlight: s.highlight,
            code: s.code,
            link: s.link.clone(),
            font_family: s.font_family.clone(),
            font_size_hp: s.font_size_hp,
            color: s.color.clone(),
            vert_align: s.vert_align,
        }
    }
}

impl From<neutral::RunStyle> for RunStyle {
    fn from(s: neutral::RunStyle) -> Self {
        RunStyle {
            bold: s.bold,
            regular: s.regular,
            italic: s.italic,
            upright: s.upright,
            caps: s.caps,
            underline: s.underline,
            strikethrough: s.strikethrough,
            highlight: s.highlight,
            code: s.code,
            link: s.link,
            font_family: s.font_family,
            font_size_hp: s.font_size_hp,
            color: s.color,
            vert_align: s.vert_align,
            ..RunStyle::default()
        }
    }
}

impl From<&ParaStyle> for neutral::ParaStyle {
    fn from(s: &ParaStyle) -> Self {
        neutral::ParaStyle {
            heading: s.heading,
            alignment: s.alignment,
            list: s.list,
            list_level: s.list_level,
            line_spacing: s.line_spacing,
            space_before_pt: s.space_before_pt,
            space_after_pt: s.space_after_pt,
            named_style: s.named_style.clone(),
        }
    }
}

impl From<neutral::ParaStyle> for ParaStyle {
    fn from(s: neutral::ParaStyle) -> Self {
        ParaStyle {
            heading: s.heading,
            alignment: s.alignment,
            list: s.list,
            list_level: s.list_level,
            line_spacing: s.line_spacing,
            space_before_pt: s.space_before_pt,
            space_after_pt: s.space_after_pt,
            named_style: s.named_style,
            ..ParaStyle::default()
        }
    }
}

impl From<&Run> for neutral::Run {
    fn from(r: &Run) -> Self {
        neutral::Run { text: r.text.clone(), style: (&r.style).into() }
    }
}

impl From<neutral::Run> for Run {
    fn from(r: neutral::Run) -> Self {
        Run { text: r.text, style: r.style.into() }
    }
}

impl From<&Paragraph> for neutral::Paragraph {
    fn from(p: &Paragraph) -> Self {
        neutral::Paragraph { style: (&p.style).into(), runs: p.runs.iter().map(Into::into).collect() }
    }
}

impl From<neutral::Paragraph> for Paragraph {
    fn from(p: neutral::Paragraph) -> Self {
        Paragraph { style: p.style.into(), runs: p.runs.into_iter().map(Into::into).collect() }
    }
}
