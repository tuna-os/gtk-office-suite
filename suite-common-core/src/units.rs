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
}
