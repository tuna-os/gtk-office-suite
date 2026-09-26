//! export.rs — the deck as PDF (a page per slide, or handouts of 2, 4 or 6
//! slides to a page) and the current slide as PNG.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Every slide is drawn by the canvas's own renderer as a show draws it
//! (`canvas::draw_slide_in`, `Chrome::Export`): the same text, shapes and
//! master as the editor, with none of its marks. The PDF is vector: cairo's
//! PDF surface keeps text as text.
//!
//! Slides are drawn on the model's 16:9 slide, 10in x 5.625in; a deck of
//! another size (`MasterSlide::page_emu`) is still exported on that page,
//! as the editor shows it.

use std::path::Path;

use crate::canvas::{draw_slide_in, Chrome};
use decks_core::engine::Deck;
use gtk4::cairo;

/// A slide page: 10in x 5.625in, in points.
pub const SLIDE_PT: (f64, f64) = (720.0, 405.0);
/// A handout page: A4 portrait, in points.
pub const HANDOUT_PT: (f64, f64) = (595.276, 841.89);

/// Draw slide `index` into `(x, y, w, h)` of `cr` (a 16:9 box).
fn draw_into(cr: &cairo::Context, deck: &Deck, index: usize, (x, y, w, h): (f64, f64, f64, f64)) -> Result<(), String> {
    cr.save().map_err(|e| e.to_string())?;
    cr.translate(x, y);
    // Drawn at the model's own 960 x 540 and scaled onto the box, with
    // nothing around the slide (a show's black surround left a line along
    // some boxes' edges in a PDF viewer).
    cr.scale(w / 960.0, h / 540.0);
    cr.rectangle(0.0, 0.0, 960.0, 540.0);
    cr.clip();
    draw_slide_in(cr, 960.0, 540.0, &deck.slides, index, &deck.masters, Chrome::Export);
    cr.restore().map_err(|e| e.to_string())
}

/// Where the slides go on a handout page of `per_page` (2, 4 or 6): one
/// column of two, or two columns of two or three rows; each a 16:9 box as
/// large as its cell allows, with room under it for its number.
pub fn handout_boxes(per_page: usize) -> Vec<(f64, f64, f64, f64)> {
    let (cols, rows) = match per_page {
        2 => (1, 2),
        4 => (2, 2),
        _ => (2, 3),
    };
    let margin = 54.0;
    let (pw, ph) = HANDOUT_PT;
    let (cw, ch) = ((pw - 2.0 * margin) / cols as f64, (ph - 2.0 * margin) / rows as f64);
    let label = 18.0;
    let gap = 18.0;
    let k = ((cw - gap) / 16.0).min((ch - gap - label) / 9.0);
    let (w, h) = (16.0 * k, 9.0 * k);
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let x = margin + c as f64 * cw + (cw - w) / 2.0;
            let y = margin + r as f64 * ch + (ch - h - label) / 2.0;
            out.push((x, y, w, h));
        }
    }
    out
}

/// The deck as a PDF at `path`: a page per slide, or with `per_page` (2, 4
/// or 6) handouts with that many slides to an A4 page, each framed and
/// numbered.
pub fn export_pdf(deck: &Deck, path: &Path, per_page: Option<usize>) -> Result<(), String> {
    let (pw, ph) = if per_page.is_some() { HANDOUT_PT } else { SLIDE_PT };
    let surface = cairo::PdfSurface::new(pw, ph, path).map_err(|e| e.to_string())?;
    let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
    match per_page {
        None => {
            for i in 0..deck.slides.len() {
                draw_into(&cr, deck, i, (0.0, 0.0, pw, ph))?;
                cr.show_page().map_err(|e| e.to_string())?;
            }
        }
        Some(n) => {
            let boxes = handout_boxes(n);
            for (page, chunk) in (0..deck.slides.len()).collect::<Vec<_>>().chunks(boxes.len()).enumerate() {
                let _ = page;
                for (i, (x, y, w, h)) in chunk.iter().zip(&boxes) {
                    draw_into(&cr, deck, *i, (*x, *y, *w, *h))?;
                    // A hairline frame, and the slide's number under it.
                    cr.set_source_rgb(0.6, 0.6, 0.6);
                    cr.set_line_width(0.5);
                    cr.rectangle(*x, *y, *w, *h);
                    cr.stroke().map_err(|e| e.to_string())?;
                    let layout = pangocairo::functions::create_layout(&cr);
                    let mut desc = pango::FontDescription::from_string("Sans");
                    desc.set_absolute_size(9.0 * pango::SCALE as f64);
                    layout.set_font_description(Some(&desc));
                    layout.set_text(&(i + 1).to_string());
                    let (tw, _) = layout.pixel_size();
                    cr.set_source_rgb(0.3, 0.3, 0.3);
                    cr.move_to(x + (w - tw as f64) / 2.0, y + h + 4.0);
                    pangocairo::functions::show_layout(&cr, &layout);
                }
                cr.show_page().map_err(|e| e.to_string())?;
            }
        }
    }
    drop(cr);
    surface.finish();
    Ok(())
}

/// Slide `index` as a PNG `width` pixels wide (16:9) at `path`.
pub fn export_png(deck: &Deck, index: usize, width: i32, path: &Path) -> Result<(), String> {
    if index >= deck.slides.len() {
        return Err("No such slide".into());
    }
    let height = width * 9 / 16;
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height).map_err(|e| e.to_string())?;
    {
        let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
        draw_into(&cr, deck, index, (0.0, 0.0, width as f64, height as f64))?;
    }
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    surface.write_to_png(&mut file).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use decks_core::engine::{SlideObject, TextBody};

    /// A deck of `n` slides, each with a title "Slide k" and a red box.
    fn deck(n: usize) -> Deck {
        let mut d = Deck::new();
        let proto = d.slides[0].clone();
        d.slides = (1..=n)
            .map(|k| {
                let mut s = proto.clone();
                s.title = format!("S{k}");
                s.objects = vec![
                    SlideObject::TextBox {
                        text: format!("Slide {k}"),
                        x: 80.0,
                        y: 40.0,
                        w: 800.0,
                        h: 90.0,
                        rotation: 0.0,
                        runs: vec![],
                        body: TextBody::default(),
                    },
                    SlideObject::Shape {
                        kind: decks_core::engine::shape::ShapeKind::Rect,
                        x: 380.0,
                        y: 250.0,
                        w: 200.0,
                        h: 150.0,
                        rotation: 0.0,
                        style: decks_core::engine::shape::ShapeStyle {
                            fill: Some(decks_core::engine::shape::Color(0xE0, 0x1B, 0x24)),
                            gradient: None,
                            stroke: None,
                        },
                    },
                ];
                s
            })
            .collect();
        d
    }

    /// `cmd` with `args`, if it is installed; in CI it has to be.
    fn tool(cmd: &str, args: &[&str]) -> Option<String> {
        match std::process::Command::new(cmd).args(args).output() {
            Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).to_string()),
            Ok(o) => panic!("{cmd} failed: {}", String::from_utf8_lossy(&o.stderr)),
            Err(_) if std::env::var_os("CI").is_some() => panic!("{cmd} is required in CI (poppler-utils)"),
            Err(_) => {
                eprintln!("skipping: {cmd} not installed");
                None
            }
        }
    }

    #[test]
    fn handout_boxes_are_16_9_on_the_page_and_do_not_overlap() {
        for n in [2, 4, 6] {
            let b = handout_boxes(n);
            assert_eq!(b.len(), n);
            for (i, (x, y, w, h)) in b.iter().enumerate() {
                assert!((w / h - 16.0 / 9.0).abs() < 1e-9);
                assert!(*x >= 0.0 && *y >= 0.0 && x + w <= HANDOUT_PT.0 && y + h + 18.0 <= HANDOUT_PT.1, "{n}: {i}");
                for (x2, y2, w2, h2) in &b[i + 1..] {
                    assert!(x + w <= *x2 || x2 + w2 <= *x || y + h <= *y2 || y2 + h2 <= *y, "{n}: boxes overlap");
                }
            }
        }
    }

    /// poppler, an independent reader, sees a page per slide at 10in x
    /// 5.625in, and each slide's text on its own page.
    #[test]
    fn a_pdf_has_a_page_per_slide_with_its_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deck.pdf");
        export_pdf(&deck(3), &path, None).unwrap();
        let p = path.to_str().unwrap();
        let Some(info) = tool("pdfinfo", &[p]) else { return };
        assert!(info.contains("Pages:           3"), "{info}");
        assert!(info.contains("Page size:       720 x 405 pts"), "{info}");
        for k in 1..=3 {
            let page = k.to_string();
            let text = tool("pdftotext", &["-f", &page, "-l", &page, p, "-"]).unwrap();
            assert!(text.contains(&format!("Slide {k}")), "page {k}: {text:?}");
        }
    }

    #[test]
    fn handouts_put_2_4_or_6_slides_to_an_a4_page() {
        for (n, pages) in [(2, 4), (4, 2), (6, 2)] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("handouts.pdf");
            export_pdf(&deck(7), &path, Some(n)).unwrap();
            // EXPORT_KEEP=<dir> keeps the files, to look at.
            if let Some(keep) = std::env::var_os("EXPORT_KEEP") {
                let _ = std::fs::copy(&path, std::path::Path::new(&keep).join(format!("handouts-{n}.pdf")));
            }
            let p = path.to_str().unwrap();
            let Some(info) = tool("pdfinfo", &[p]) else { return };
            assert!(info.contains(&format!("Pages:           {pages}")), "{n} per page: {info}");
            assert!(info.contains("(A4)"), "{n} per page: {info}");
            // The first page holds the first n slides, in order.
            let text = tool("pdftotext", &["-f", "1", "-l", "1", p, "-"]).unwrap();
            let found: Vec<usize> = (1..=7).filter(|k| text.contains(&format!("Slide {k}"))).collect();
            assert_eq!(found, (1..=n).collect::<Vec<_>>(), "{n} per page: {text:?}");
        }
    }

    /// The PDF page, rasterised by poppler, is the slide the PNG export
    /// draws: the same renderer, one as vector and one as pixels. Measured
    /// as the red box's pixel count. The box is 200 x 150 units at a whole
    /// pixel offset, so 30000 pixels exactly; the bound below allows its
    /// one-pixel edge ring (700 pixels) to fall under the "strong red" test
    /// through antialiasing, and nothing more.
    #[test]
    fn a_pdf_page_and_the_png_show_the_same_slide() {
        let dir = tempfile::tempdir().unwrap();
        let (pdf, png) = (dir.path().join("deck.pdf"), dir.path().join("slide.png"));
        let d = deck(1);
        export_pdf(&d, &pdf, None).unwrap();
        export_png(&d, 0, 960, &png).unwrap();
        let prefix = dir.path().join("page");
        let Some(_) = tool("pdftoppm", &["-png", "-r", "96", "-singlefile", pdf.to_str().unwrap(), prefix.to_str().unwrap()]) else { return };
        let red = |path: &Path| -> (u32, u32, usize) {
            let s = cairo::ImageSurface::create_from_png(&mut std::fs::File::open(path).unwrap()).unwrap();
            let (w, h, stride) = (s.width() as usize, s.height() as usize, s.stride() as usize);
            let data = s.take_data().unwrap();
            let mut n = 0;
            for y in 0..h {
                for x in 0..w {
                    let p = &data[y * stride + x * 4..y * stride + x * 4 + 4];
                    // BGRA: strong red, little green and blue.
                    if p[2] > 180 && p[1] < 80 && p[0] < 80 {
                        n += 1;
                    }
                }
            }
            (w as u32, h as u32, n)
        };
        let (pw, ph, pr) = red(&prefix.with_extension("png"));
        let (sw, sh, sr) = red(&png);
        assert_eq!((pw, ph), (sw, sh), "96 dpi of 10in x 5.625in is 960 x 540");
        assert!((29_300..=30_000).contains(&sr), "PNG: {sr}");
        assert!((29_300..=30_000).contains(&pr), "PDF: {pr}");
    }
}
