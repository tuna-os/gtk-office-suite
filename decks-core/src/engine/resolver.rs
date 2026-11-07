// resolver.rs — the one precedence resolver (DCK-006): what a slide's
// background, an object's fill and outline, and a text run's font are
// once every level has had its say.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Precedence, highest first:
//
//   1. the object's own statement (a shape's or text frame's `fill_src` /
//      `stroke_src`, a run's `font_family`); a slide's own `background`;
//   2. the slide's layout (`Layout::background`);
//   3. the slide's master (`MasterSlide::background`, `default_font`);
//   4. the master's theme: a retained theme found by `MasterSlide::theme_id`
//      (else the documented default theme), a presentation slot (`tx1`,
//      `bg1` …) remapped through `MasterSlide::color_map` before the
//      theme's slots, then the source modulation applied
//      (`ColorMod::apply_all`); the theme's major/minor typefaces;
//   5. the documented fallbacks: `layouts::FALLBACK_BACKGROUND` and
//      `MasterSlide::DEFAULT_FONT`. Shapes have none: an unspecified fill
//      or outline is no fill and no outline (`ShapeStyle`), never a default.
//
// Absence is not a value (DCK-006): an empty background or font at one
// level means "inherit", and an explicit `a:noFill`/no-line (`FillSrc::NoFill`,
// `StrokeSrc::NoLine`) stops the walk with "none" instead of inheriting.
// A direct reference is resolved live against the slide's master's theme,
// so a theme edit reaches every inheritor while the resolved cache beside
// the reference (`ShapeStyle::fill`) is only consulted when the reference
// names nothing the theme answers (an unknown token): the colour the file
// last drew, not an invented one.
//
// Read-only by construction: the resolver borrows the deck and never
// touches it or its history. It is the only place this precedence is
// written down; `layouts::resolve_background` delegates here.
//
// What the model does not retain yet, so no level here can answer it:
// placeholder-level fill/stroke/font on layouts and masters (the readers
// bake a placeholder's inherited style into the slide object at import,
// and `LayoutPlaceholder` keeps geometry and source key only). Those
// levels join this chain when the model stores them; nothing here
// should invent them in the meantime.

use std::borrow::Cow;
use std::sync::LazyLock;

use super::shape::{Color, ShapeStyle, Stroke};
use super::theme::{ColorMod, ColorRef, FillSrc, StrokeSrc, Theme};
use super::{Deck, MasterSlide, Slide, SlideObject};
use crate::layouts::{Placeholder, FALLBACK_BACKGROUND};

/// The default Office theme a master with no retained theme resolves
/// against, built once from [`Theme::default_colors`].
static DEFAULT_THEME: LazyLock<Theme> = LazyLock::new(|| Theme {
    id: 0,
    name: "Office".into(),
    colors: Theme::default_colors(),
    major_font: None,
    minor_font: None,
    source: String::new(),
});

/// The first background any of the three levels states, in precedence
/// order: the slide's own, then its layout's, then its master's; empty
/// means "states none" at every level. The pure half of the precedence,
/// shared by [`PrecedenceResolver::background`] and
/// `layouts::resolve_background` so there is one definition of it.
pub(crate) fn stated_background<'a>(slide_bg: &'a str, layout_bg: Option<&'a str>, master_bg: &'a str) -> Option<&'a str> {
    if !slide_bg.is_empty() {
        return Some(slide_bg);
    }
    if let Some(bg) = layout_bg.filter(|b| !b.is_empty()) {
        return Some(bg);
    }
    if !master_bg.is_empty() {
        return Some(master_bg);
    }
    None
}

/// Resolves effective appearance for one deck, level by level. Cheap to
/// construct (one borrow); make one per query batch.
pub struct PrecedenceResolver<'a> {
    deck: &'a Deck,
}

impl<'a> PrecedenceResolver<'a> {
    pub fn new(deck: &'a Deck) -> Self {
        PrecedenceResolver { deck }
    }

    /// The theme `master` resolves against: its retained theme, else the
    /// documented default theme.
    fn theme(&self, master: Option<&MasterSlide>) -> &'a Theme {
        master.and_then(|m| self.deck.theme_of(m)).unwrap_or(&DEFAULT_THEME)
    }

    /// `r` as drawn under `master`'s theme: a literal as itself, a theme
    /// slot remapped through the master's colour map, looked up in the
    /// theme and modulated as the source states. `None` when the token
    /// names a slot neither the theme nor the default theme has.
    pub fn color(&self, master: Option<&MasterSlide>, r: &ColorRef) -> Option<Color> {
        match r {
            ColorRef::Rgb(hex) => Color::from_hex(hex),
            ColorRef::Theme(t) => {
                let slot = master.and_then(|m| m.color_map.get(&t.slot)).map_or(t.slot.as_str(), String::as_str);
                self.theme(master).slot(slot).map(|c| ColorMod::apply_all(&t.mods, c))
            }
        }
    }

    /// The slide's background as `#rrggbb`-style text: its own, else its
    /// layout's, else its master's, else the master's theme `bg1` (through
    /// the colour map), else [`FALLBACK_BACKGROUND`]. A slide's background
    /// picture is a separate layer drawn over this colour and is not
    /// resolved here.
    pub fn background(&self, slide: &'a Slide) -> Cow<'a, str> {
        let master = self.deck.master_of(slide);
        let layout = self.deck.layout_of(slide);
        let stated = stated_background(
            &slide.background,
            layout.and_then(|l| l.background.as_deref()),
            master.map_or("", |m| m.background.as_str()),
        );
        if let Some(bg) = stated {
            return Cow::Borrowed(bg);
        }
        match self.color(master, &ColorRef::Theme(super::theme::ThemeColor::unmodulated("bg1"))) {
            Some(c) => Cow::Owned(format!("#{}", c.to_hex().to_lowercase())),
            None => Cow::Borrowed(FALLBACK_BACKGROUND),
        }
    }

    /// The shape style `obj` paints with, if it has one: a preset shape's,
    /// or the frame a text box sits in.
    fn style_of(obj: &SlideObject) -> Option<&ShapeStyle> {
        match obj {
            SlideObject::Shape { style, .. } => Some(style),
            SlideObject::TextBox { body, .. } => body.frame.as_ref().map(|f| &f.style),
            _ => None,
        }
    }

    /// The fill `obj` is drawn with on `slide`: its own statement (a
    /// token resolved live under the slide's master's theme, a literal,
    /// or an explicit no-fill that stops here), else the resolved cache
    /// it was imported with, else none. `None` is no fill — shapes have
    /// no inherited or fallback fill to apply.
    pub fn fill(&self, slide: &Slide, obj: &SlideObject) -> Option<Color> {
        let style = Self::style_of(obj)?;
        let master = self.deck.master_of(slide);
        match &style.fill_src {
            Some(FillSrc::NoFill) => None,
            Some(FillSrc::Color(r)) => self.color(master, r).or(style.fill),
            None => style.fill,
        }
    }

    /// The outline `obj` is drawn with on `slide`, resolved like
    /// [`fill`](Self::fill). A token's width is the cached outline's, else
    /// one model unit (what `ShapeStyle::default` draws).
    pub fn stroke(&self, slide: &Slide, obj: &SlideObject) -> Option<Stroke> {
        let style = Self::style_of(obj)?;
        let master = self.deck.master_of(slide);
        let width = style.stroke.map_or(1.0, |s| s.width);
        match &style.stroke_src {
            Some(StrokeSrc::NoLine) => None,
            Some(StrokeSrc::Color(r)) => match self.color(master, r) {
                Some(color) => Some(Stroke { color, width }),
                None => style.stroke,
            },
            None => style.stroke,
        }
    }

    /// The font family text is set in: `local` (a run's or box's own, when
    /// not blank), else the master's, else its theme's, else
    /// [`MasterSlide::DEFAULT_FONT`]. A title (`role`) takes the theme's
    /// heading face ahead of the master's body font; every other text
    /// takes the master's `default_font` — the body font its theme gave
    /// it at import, or one the user set — ahead of the theme's body face.
    pub fn font<'s>(&'s self, slide: &Slide, role: Option<Placeholder>, local: Option<&'s str>) -> &'s str {
        let named = |s: &'s str| {
            let s = s.trim();
            (!s.is_empty()).then_some(s)
        };
        if let Some(f) = local.and_then(named) {
            return f;
        }
        let master = self.deck.master_of(slide);
        let theme = master.and_then(|m| self.deck.theme_of(m));
        let major = theme.and_then(|t| t.major_font.as_deref()).and_then(named);
        let minor = theme.and_then(|t| t.minor_font.as_deref()).and_then(named);
        let master_font = master.and_then(|m| named(m.default_font.as_str()));
        match role {
            Some(Placeholder::Title) => major.or(master_font).or(minor),
            _ => master_font.or(minor),
        }
        .unwrap_or(MasterSlide::DEFAULT_FONT)
    }
}
