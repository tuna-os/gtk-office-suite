// print.rs — Shared print/page-setup infrastructure.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice svl/gridprinter.hxx.
// Page size, margins, orientation for all three apps.
// Used by Letters (document pages), Tables (sheet to pages),
// and Decks (slides as pages).

/// Standard page sizes in millimeters (ISO 216).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Orientation { Portrait, Landscape }

/// The smallest and largest print scale Calc and Excel accept (10–400%).
pub const MIN_PRINT_SCALE: f64 = 0.10;
pub const MAX_PRINT_SCALE: f64 = 4.0;

/// Page setup configuration.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PageSetup {
    pub size: PageSize,
    pub orientation: Orientation,
    pub margin_top_mm: f64,
    pub margin_bottom_mm: f64,
    pub margin_left_mm: f64,
    pub margin_right_mm: f64,
    pub scale: f64, // 1.0 = 100%
    /// Fit the print to at most this many pages (wide, tall); 0 means no
    /// limit in that direction. `None` prints at `scale`.
    #[serde(default)]
    pub fit_to_pages: Option<(u32, u32)>,
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
            fit_to_pages: None,
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

    /// The scale a printout whose content measures `content_w_mm` ×
    /// `content_h_mm` at 100% is drawn at, as Calc and Excel print it.
    ///
    /// With [`Self::fit_to_pages`] set, the content is shrunk until it fits
    /// that many pages' printable area in each direction that has a limit
    /// (`0` means none), and never enlarged: a sheet that already fits
    /// prints at 100%, whatever `scale` says. Otherwise it prints at
    /// `scale`. Either way the result stays within the 10–400% spreadsheets
    /// accept, and a scale that is not a positive number prints at 100%.
    ///
    /// The fit treats the content as one block, which is exact for one page
    /// each way; across several pages Calc also keeps whole columns and
    /// rows together, so its scale can be a little smaller.
    pub fn print_scale_for(&self, content_w_mm: f64, content_h_mm: f64) -> f64 {
        let Some((wide, tall)) = self.fit_to_pages else {
            return if self.scale.is_finite() && self.scale > 0.0 {
                self.scale.clamp(MIN_PRINT_SCALE, MAX_PRINT_SCALE)
            } else {
                1.0
            };
        };
        let (area_w, area_h) = self.content_area_mm();
        let fit = |pages: u32, area: f64, content: f64| {
            // Nothing to fit, or no room to fit it in (a NaN counts as
            // either): no limit this way.
            if pages == 0 || content.is_nan() || content <= 0.0 || area.is_nan() || area <= 0.0 {
                f64::INFINITY
            } else {
                f64::from(pages) * area / content
            }
        };
        fit(wide, area_w, content_w_mm)
            .min(fit(tall, area_h, content_h_mm))
            .clamp(MIN_PRINT_SCALE, 1.0)
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

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn print_scale_is_the_stored_scale_without_a_fit() {
        let setup = PageSetup { scale: 0.46, ..Default::default() };
        assert!(close(setup.print_scale_for(1000.0, 1000.0), 0.46));
        assert!(close(PageSetup::default().print_scale_for(1000.0, 1000.0), 1.0));
    }

    #[test]
    fn print_scale_stays_within_what_spreadsheets_accept() {
        let tiny = PageSetup { scale: 0.01, ..Default::default() };
        let huge = PageSetup { scale: 9.0, ..Default::default() };
        let nan = PageSetup { scale: f64::NAN, ..Default::default() };
        let zero = PageSetup { scale: 0.0, ..Default::default() };
        assert!(close(tiny.print_scale_for(10.0, 10.0), MIN_PRINT_SCALE));
        assert!(close(huge.print_scale_for(10.0, 10.0), MAX_PRINT_SCALE));
        assert!(close(nan.print_scale_for(10.0, 10.0), 1.0));
        assert!(close(zero.print_scale_for(10.0, 10.0), 1.0));
    }

    /// Fit to one page: the tighter direction decides, and the stored
    /// scale is ignored, as Calc ignores it while "fit" is on.
    #[test]
    fn fit_to_one_page_shrinks_by_the_tighter_direction() {
        // Default A4 portrait printable area is 159.2 × 246.2 mm.
        let setup = PageSetup { scale: 0.62, fit_to_pages: Some((1, 1)), ..Default::default() };
        assert!(close(setup.print_scale_for(318.4, 246.2), 0.5));
        assert!(close(setup.print_scale_for(159.2, 984.8), 0.25));
    }

    #[test]
    fn fit_to_width_leaves_the_height_unlimited() {
        let setup = PageSetup { fit_to_pages: Some((1, 0)), ..Default::default() };
        assert!(close(setup.print_scale_for(318.4, 99_999.0), 0.5));
    }

    #[test]
    fn fit_to_two_pages_wide_allows_twice_the_width() {
        let setup = PageSetup { fit_to_pages: Some((2, 0)), ..Default::default() };
        assert!(close(setup.print_scale_for(636.8, 10.0), 0.5));
    }

    #[test]
    fn fit_never_enlarges_content_that_already_fits() {
        let setup = PageSetup { scale: 2.0, fit_to_pages: Some((1, 1)), ..Default::default() };
        assert!(close(setup.print_scale_for(50.0, 50.0), 1.0));
        // No limit either way, or nothing to print: 100%.
        let none = PageSetup { fit_to_pages: Some((0, 0)), ..Default::default() };
        assert!(close(none.print_scale_for(5000.0, 5000.0), 1.0));
        let one = PageSetup { fit_to_pages: Some((1, 1)), ..Default::default() };
        assert!(close(one.print_scale_for(0.0, 0.0), 1.0));
    }

    #[test]
    fn fit_bottoms_out_at_ten_percent() {
        let setup = PageSetup { fit_to_pages: Some((1, 1)), ..Default::default() };
        assert!(close(setup.print_scale_for(1.0e6, 10.0), MIN_PRINT_SCALE));
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
