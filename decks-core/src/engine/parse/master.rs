// Slide-master and layout decoration parsing for the PPTX reader.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
/// The `p:cSld/@name` of a slideMaster or slideLayout part.
///
/// This is the master's own name, and it is what the writer puts there.
/// Without it the name fell out of the part's file path — "slideLayout1"
/// for a deck we wrote, so a master called "House Style" came back under a
/// name nobody chose, and the sidebar showed it.
pub fn parse_c_sld_name(xml: &str) -> Option<String> {
    if xml.is_empty() {
        return None;
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    while let Ok(ev) = reader.read_event_into(&mut buf) {
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) if e.name().as_ref() == "p:cSld" => {
                return e
                    .attributes()
                    .filter_map(|a| a.ok())
                    .find(|a| a.key.as_ref() == "name")
                    // Normalised, not raw: a name containing `&` or `<` is
                    // escaped in the attribute, and reading the bytes back
                    // verbatim returns "R&amp;D" as the name itself.
                    .and_then(|a| {
                        a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .ok()
                            .map(|v| v.to_string())
                    })
                    .filter(|n| !n.is_empty());
            }
            Event::Eof => break,
            _ => {}
        }
    }
    None
}

/// Parse a slideMaster/slideLayout part: background color and
/// non-placeholder decoration shapes. Placeholder shapes (`p:ph` —
/// "Click to edit Master title style" and friends) are styling slots,
/// not content, and are skipped.
pub fn parse_master_shapes(xml: &str) -> (Option<String>, Vec<SlideObject>) {
    parse_master_shapes_scaled(xml, SlideScale::default())
}

/// As `parse_master_shapes`, against the slide size the package declares.
pub fn parse_master_shapes_scaled(
    xml: &str,
    scale: SlideScale,
) -> (Option<String>, Vec<SlideObject>) {
    master_shapes(xml, scale, &Theme::default(), &mut |_| None)
}
/// A master's or layout's background and decorations. A shape without text
/// is read with its preset and paint (fill, gradient, outline, resolved
/// against `theme`), as on a slide: they used to come back as plain
/// rectangles and circles, so a themed master lost its decorations on
/// every save and reopen.
pub(super) fn master_shapes(
    xml: &str,
    scale: SlideScale,
    theme: &Theme,
    pictures: &mut dyn FnMut(&str) -> Option<String>,
) -> (Option<String>, Vec<SlideObject>) {
    if xml.is_empty() {
        return (None, Vec::new());
    }
    let mut reader = Reader::from_str(xml);
    // Untrimmed for the same reason as the slide walker above.
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut background: Option<String> = None;
    let mut shapes = Vec::new();
    let mut in_bg = false;
    let mut in_text = false;
    let mut in_rpr = false;
    // (x, y, w, h, prst, has_ph, text)
    struct Pending {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        prst: Option<String>,
        has_ph: bool,
        runs: Vec<Run>,
        pending_breaks: usize,
        cur_style: RunStyle,
        /// Which `p:sp` of the part this is, in document order: its index
        /// into `sp_styles`.
        sp_index: usize,
        /// A picture (`p:pic`) rather than a shape, and its image's
        /// relationship.
        picture: bool,
        embed: Option<String>,
    }
    let paints = sp_styles(xml, theme, scale.x);
    let mut sp_count = 0usize;
    impl Pending {
        fn push_run(&mut self, mut text: String) {
            // Leading blank paragraphs keep their lines, as on a slide.
            if self.runs.is_empty() {
                text.insert_str(0, &"\n".repeat(self.pending_breaks));
            }
            close_paragraphs(&mut self.runs, self.pending_breaks);
            self.pending_breaks = 0;
            self.runs.push(Run { text, style: self.cur_style.clone() });
        }
    }
    let mut cur: Option<Pending> = None;
    while let Ok(ev) = reader.read_event_into(&mut buf) {
        match ev {
            // `<a:p/>` arrives as one Empty event rather than Start+End, so
            // the End arm below never sees it — and an empty paragraph is
            // exactly the blank line that must not be swallowed.
            Event::Empty(ref e) if e.name().as_ref() == "a:p" => {
                if let Some(p) = cur.as_mut() {
                    p.pending_breaks += 1;
                }
            }
            Event::Start(ref e) | Event::Empty(ref e) => match e.name().as_ref() {
                "p:bg" => in_bg = true,
                // A line break inside the paragraph, as on a slide.
                "a:br" => {
                    if let Some(p) = cur.as_mut() {
                        if p.pending_breaks == 0 && !p.runs.is_empty() {
                            if let Some(r) = p.runs.last_mut() {
                                r.text.push(super::LINE_BREAK);
                            }
                        } else {
                            p.push_run(super::LINE_BREAK.to_string());
                        }
                    }
                }
                "p:sp" => {
                    cur = Some(Pending {
                        x: 0.0,
                        y: 0.0,
                        w: 0.0,
                        h: 0.0,
                        prst: None,
                        has_ph: false,
                        runs: Vec::new(),
                        pending_breaks: 0,
                        cur_style: RunStyle::default(),
                        sp_index: sp_count,
                        picture: false,
                        embed: None,
                    });
                    sp_count += 1;
                }
                "p:pic" => {
                    cur = Some(Pending {
                        x: 0.0,
                        y: 0.0,
                        w: 0.0,
                        h: 0.0,
                        prst: None,
                        has_ph: false,
                        runs: Vec::new(),
                        pending_breaks: 0,
                        cur_style: RunStyle::default(),
                        sp_index: usize::MAX,
                        picture: true,
                        embed: None,
                    });
                }
                "a:blip" => {
                    if let Some(p) = cur.as_mut().filter(|p| p.picture) {
                        p.embed = parse_blip_embed(e);
                    }
                }
                "p:ph" => {
                    if let Some(p) = cur.as_mut() {
                        p.has_ph = true;
                    }
                }
                "a:off" => {
                    if let Some(p) = cur.as_mut() {
                        let (x, y) = parse_coords(e, "x", "y");
                        if let Some(x) = x {
                            p.x = x * scale.x;
                        }
                        if let Some(y) = y {
                            p.y = y * scale.y;
                        }
                    }
                }
                "a:ext" => {
                    if let Some(p) = cur.as_mut() {
                        let (w, h) = parse_coords(e, "cx", "cy");
                        if let Some(w) = w {
                            p.w = w * scale.x;
                        }
                        if let Some(h) = h {
                            p.h = h * scale.y;
                        }
                    }
                }
                "a:prstGeom" => {
                    if let Some(p) = cur.as_mut() {
                        p.prst = parse_prst_geom(e);
                    }
                }
                "a:t" => in_text = true,
                "a:rPr" => {
                    if let Some(p) = cur.as_mut() {
                        p.cur_style = parse_run_style(e);
                    }
                    in_rpr = true;
                }
                "a:srgbClr" if in_rpr => {
                    if let Some(p) = cur.as_mut() {
                        if let Some(val) = e
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == "val")
                        {
                            // Lowercase and without the '#', matching the
                            // slide walker and `RunStyle`'s documented form.
                            p.cur_style.color = Some(val.value.to_lowercase());
                        }
                    }
                }
                "a:srgbClr" if in_bg => {
                    if let Some(val) = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == "val")
                    {
                        background =
                            Some(format!("#{}", val.value.to_lowercase()));
                    }
                }
                // A theme-slot background, resolved against the part's
                // own master's theme (the caller passes it remapped).
                "a:schemeClr" if in_bg => {
                    if let Some(val) = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == "val")
                    {
                        if let Some(color) = theme.slot(&val.value.to_lowercase()) {
                            background = Some(format!("#{}", color.to_hex()));
                        }
                    }
                }
                _ => {}
            },
            Event::End(ref e) => match e.name().as_ref() {
                "p:pic" => {
                    // A master's or layout's picture (a logo, a band of
                    // colour), where it stands among its shapes.
                    if let Some(p) = cur.take() {
                        if let Some(path) = p.embed.as_deref().filter(|_| !p.has_ph && p.w > 0.0 && p.h > 0.0).and_then(&mut *pictures) {
                            shapes.push(SlideObject::Image { path, x: p.x, y: p.y, w: p.w, h: p.h, rotation: 0.0, crop: Default::default() });
                        }
                    }
                }
                "p:bg" => in_bg = false,
                "a:t" => in_text = false,
                "a:rPr" => in_rpr = false,
                "a:p" => {
                    if let Some(p) = cur.as_mut() {
                        p.pending_breaks += 1;
                    }
                }
                "p:sp" => {
                    if let Some(p) = cur.take() {
                        if !p.has_ph && p.w > 0.0 && p.h > 0.0 {
                            let text = text_of(&p.runs);
                            let has_text = !text.trim().is_empty();
                            if has_text {
                                shapes.push(SlideObject::TextBox {
                                    text,
                                    x: p.x,
                                    y: p.y,
                                    w: p.w,
                                    h: p.h,
                                    rotation: 0.0,
                                    runs: p.runs,
                                    body: Default::default(),
                                });
                            } else {
                                let paint = paints.get(p.sp_index).cloned().unwrap_or_default();
                                let mut kind = ShapeKind::from_prst(p.prst.as_deref().unwrap_or("rect"));
                                if let (ShapeKind::RoundRect { radius }, Some(adj)) = (&mut kind, paint.round_adj) {
                                    *radius = adj.clamp(0.0, 0.5);
                                }
                                shapes.push(SlideObject::Shape {
                                    kind,
                                    x: p.x,
                                    y: p.y,
                                    w: p.w,
                                    h: p.h,
                                    rotation: 0.0,
                                    style: paint.style,
                                });
                            }
                        }
                    }
                }
                _ => {}
            },
            Event::Text(ref t) => {
                if in_text {
                    if let Some(p) = cur.as_mut() {
                        p.push_run(unescape_text(t));
                    }
                }
            }
            Event::GeneralRef(ref r) => {
                if in_text {
                    if let Some(p) = cur.as_mut() {
                        p.push_run(resolve_general_ref(r));
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    (background, shapes)
}
