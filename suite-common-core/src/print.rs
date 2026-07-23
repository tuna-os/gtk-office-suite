// print.rs — Shared print/page-setup infrastructure.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice svl/gridprinter.hxx.
// Page size, margins, orientation for all three apps.
// Used by Letters (document pages), Tables (sheet to pages),
// and Decks (slides as pages).

/// Standard page sizes in millimeters (ISO 216).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageSize {
    A4,
    A3,
    Letter,
    Legal,
    Custom { width_mm: f64, height_mm: f64 },
}

impl PageSize {
    pub fn dimensions_mm(&self) -> (f64, f64) {
        match self {
            PageSize::A4 => (210.0, 297.0),
            PageSize::A3 => (297.0, 420.0),
            PageSize::Letter => (215.9, 279.4),
            PageSize::Legal => (215.9, 355.6),
            PageSize::Custom { width_mm, height_mm } => (*width_mm, *height_mm),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Orientation { Portrait, Landscape }

/// Page setup configuration.
#[derive(Clone, Debug)]
pub struct PageSetup {
    pub size: PageSize,
    pub orientation: Orientation,
    pub margin_top_mm: f64,
    pub margin_bottom_mm: f64,
    pub margin_left_mm: f64,
    pub margin_right_mm: f64,
    pub scale: f64, // 1.0 = 100%
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup {
            size: PageSize::A4,
            orientation: Orientation::Portrait,
            margin_top_mm: 25.4,
            margin_bottom_mm: 25.4,
            margin_left_mm: 25.4,
            margin_right_mm: 25.4,
            scale: 1.0,
        }
    }
}

impl PageSetup {
    /// Usable content area in mm (page minus margins).
    pub fn content_area_mm(&self) -> (f64, f64) {
        let (pw, ph) = self.size.dimensions_mm();
        let (w, h) = match self.orientation {
            Orientation::Portrait => (pw, ph),
            Orientation::Landscape => (ph, pw),
        };
        (w - self.margin_left_mm - self.margin_right_mm,
         h - self.margin_top_mm - self.margin_bottom_mm)
    }

    /// Page dimensions in mm accounting for orientation.
    pub fn page_dimensions_mm(&self) -> (f64, f64) {
        let (pw, ph) = self.size.dimensions_mm();
        match self.orientation {
            Orientation::Portrait => (pw, ph),
            Orientation::Landscape => (ph, pw),
        }
    }
}

/// Compute how many pages are needed to fit content.
pub fn pages_for_content(content_width_mm: f64, content_height_mm: f64, setup: &PageSetup) -> (u32, u32) {
    let (area_w, area_h) = setup.content_area_mm();
    let cols = (content_width_mm / area_w).ceil() as u32;
    let rows = (content_height_mm / area_h).ceil() as u32;
    (cols.max(1), rows.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a4_portrait() {
        let (w, h) = PageSize::A4.dimensions_mm();
        assert!((w - 210.0).abs() < 0.1);
        assert!((h - 297.0).abs() < 0.1);
    }

    #[test]
    fn test_content_area() {
        let setup = PageSetup::default();
        let (w, h) = setup.content_area_mm();
        // A4 portrait: 210 - 50.8 = 159.2, 297 - 50.8 = 246.2
        assert!((w - 159.2).abs() < 0.5);
        assert!((h - 246.2).abs() < 0.5);
    }

    #[test]
    fn test_landscape_swaps_dimensions() {
        let setup = PageSetup { orientation: Orientation::Landscape, ..Default::default() };
        let (w, h) = setup.page_dimensions_mm();
        assert!((w - 297.0).abs() < 0.1); // swapped
        assert!((h - 210.0).abs() < 0.1);
    }

    #[test]
    fn test_pages_for_content_fits_single_page() {
        let setup = PageSetup::default();
        let (cols, rows) = pages_for_content(100.0, 200.0, &setup);
        assert_eq!(cols, 1);
        assert_eq!(rows, 1);
    }

    #[test]
    fn test_pages_for_content_spans_multiple() {
        let setup = PageSetup::default();
        let (cols, rows) = pages_for_content(500.0, 500.0, &setup);
        assert!(cols > 1);
        assert!(rows > 1);
    }
}
