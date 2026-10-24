// units.rs — Unit conversion utilities.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice svl/converter.hxx.
// Conversions between mm, pixels, twips, cm, inches, points.
// Used by Letters ruler, Tables column widths, page margins.

/// Convert millimeters to pixels at a given DPI.
pub fn mm_to_px(mm: f64, dpi: f64) -> f64 {
    mm * dpi / 25.4
}

/// Convert pixels to millimeters at a given DPI.
pub fn px_to_mm(px: f64, dpi: f64) -> f64 {
    px * 25.4 / dpi
}

/// Convert twips to pixels. 1 twip = 1/20 point = 1/1440 inch.
pub fn twips_to_px(twips: f64, dpi: f64) -> f64 {
    twips * dpi / 1440.0
}

/// Convert pixels to twips.
pub fn px_to_twips(px: f64, dpi: f64) -> f64 {
    px * 1440.0 / dpi
}

/// Convert inches to millimeters.
pub fn inches_to_mm(inches: f64) -> f64 {
    inches * 25.4
}

/// Convert millimeters to inches.
pub fn mm_to_inches(mm: f64) -> f64 {
    mm / 25.4
}

/// Convert centimeters to pixels.
pub fn cm_to_px(cm: f64, dpi: f64) -> f64 {
    mm_to_px(cm * 10.0, dpi)
}

/// Convert points to pixels. 1 point = 1/72 inch.
pub fn pt_to_px(pt: f64, dpi: f64) -> f64 {
    pt * dpi / 72.0
}

/// Standard DPI values.
pub const SCREEN_DPI: f64 = 96.0;
pub const PRINT_DPI: f64 = 300.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mm_px_roundtrip() {
        let mm = 25.4;
        let px = mm_to_px(mm, SCREEN_DPI);
        assert!((px - 96.0).abs() < 1.0);
        let back = px_to_mm(px, SCREEN_DPI);
        assert!((back - mm).abs() < 1.0);
    }

    #[test]
    fn test_twips_px() {
        // 1440 twips = 1 inch = 96 px at screen DPI
        let px = twips_to_px(1440.0, SCREEN_DPI);
        assert!((px - 96.0).abs() < 1.0);
    }

    #[test]
    fn test_inches_mm() {
        let mm = inches_to_mm(1.0);
        assert!((mm - 25.4).abs() < 0.1);
    }

    #[test]
    fn test_pt_to_px() {
        // 72pt = 1 inch = 96px
        let px = pt_to_px(72.0, SCREEN_DPI);
        assert!((px - 96.0).abs() < 1.0);
    }

    // Tight enough that a wrong conversion factor fails, loose enough for
    // the rounding of a division by 25.4.
    const EPS: f64 = 1e-9;

    /// One inch is the same length at any DPI; only its pixel count moves.
    #[test]
    fn test_one_inch_at_screen_and_print_dpi() {
        for (dpi, px_per_inch) in [(SCREEN_DPI, 96.0), (PRINT_DPI, 300.0)] {
            assert!((twips_to_px(1440.0, dpi) - px_per_inch).abs() < EPS);
            assert!((pt_to_px(72.0, dpi) - px_per_inch).abs() < EPS);
            assert!((mm_to_px(25.4, dpi) - px_per_inch).abs() < EPS);
            assert!((cm_to_px(2.54, dpi) - px_per_inch).abs() < EPS);
            assert!((px_to_twips(px_per_inch, dpi) - 1440.0).abs() < EPS);
            assert!((px_to_mm(px_per_inch, dpi) - 25.4).abs() < EPS);
        }
        assert!((mm_to_inches(25.4) - 1.0).abs() < EPS);
        assert!((inches_to_mm(1.0) - 25.4).abs() < EPS);
    }

    #[test]
    fn test_px_twips_inverse() {
        for dpi in [SCREEN_DPI, PRINT_DPI] {
            for px in [0.0, 1.0, 37.5, 96.0, 1234.75] {
                assert!((twips_to_px(px_to_twips(px, dpi), dpi) - px).abs() < EPS);
            }
            for twips in [0.0, 1.0, 567.25, 1440.0, 20_000.0] {
                assert!((px_to_twips(twips_to_px(twips, dpi), dpi) - twips).abs() < EPS);
            }
        }
        // A known pair, so a wrong 1440 factor fails rather than cancelling out.
        assert!((px_to_twips(96.0, SCREEN_DPI) - 1440.0).abs() < EPS);
        assert!((px_to_twips(150.0, PRINT_DPI) - 720.0).abs() < EPS);
    }

    #[test]
    fn test_mm_inches_inverse() {
        for mm in [0.0, 0.5, 25.4, 123.456, 297.0] {
            assert!((inches_to_mm(mm_to_inches(mm)) - mm).abs() < EPS);
        }
        for inches in [0.0, 0.25, 1.0, 8.5, 11.0] {
            assert!((mm_to_inches(inches_to_mm(inches)) - inches).abs() < EPS);
        }
        // A4 width and US Letter width, each to the other unit.
        assert!((mm_to_inches(210.0) - 210.0 / 25.4).abs() < EPS);
        assert!((inches_to_mm(8.5) - 215.9).abs() < EPS);
    }

    #[test]
    fn test_zero_and_fractional_inputs() {
        for dpi in [SCREEN_DPI, PRINT_DPI] {
            assert!(mm_to_px(0.0, dpi).abs() < EPS);
            assert!(px_to_mm(0.0, dpi).abs() < EPS);
            assert!(twips_to_px(0.0, dpi).abs() < EPS);
            assert!(px_to_twips(0.0, dpi).abs() < EPS);
            assert!(cm_to_px(0.0, dpi).abs() < EPS);
            assert!(pt_to_px(0.0, dpi).abs() < EPS);
        }
        assert!(mm_to_inches(0.0).abs() < EPS);
        assert!(inches_to_mm(0.0).abs() < EPS);
        // Half an inch at screen DPI, by each route into pixels.
        assert!((mm_to_px(12.7, SCREEN_DPI) - 48.0).abs() < EPS);
        assert!((cm_to_px(1.27, SCREEN_DPI) - 48.0).abs() < EPS);
        assert!((twips_to_px(720.0, SCREEN_DPI) - 48.0).abs() < EPS);
        assert!((pt_to_px(36.0, SCREEN_DPI) - 48.0).abs() < EPS);
        // And at print DPI, where the same length is 150 px.
        assert!((mm_to_px(12.7, PRINT_DPI) - 150.0).abs() < EPS);
        assert!((cm_to_px(1.27, PRINT_DPI) - 150.0).abs() < EPS);
        assert!((px_to_mm(150.0, PRINT_DPI) - 12.7).abs() < EPS);
    }
}
