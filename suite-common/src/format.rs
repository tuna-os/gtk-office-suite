// format.rs — Shared number formatting engine for suite-common.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice svl/ SvNumberFormatter + SvNumberformatInfo.
// Formats numbers/dates/currencies/percentages for display in Tables cells,
// Letters fields, and Decks text boxes.

use chrono::{NaiveDate, NaiveDateTime};
use num_format::{Locale, ToFormattedString};

// ── Format kind ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum NumberFormatKind {
    /// Default — display value as-is.
    General,
    /// Fixed decimal places: Number(2) → "1,234.56".
    Number(u8),
    /// Currency with symbol: Currency("$", 2) → "$1,234.56".
    Currency(String, u8),
    /// Percentage: Percent(1) → "12.3%" (value 0.123 becomes 12.3%).
    Percent(u8),
    /// Date from Excel serial or ISO string: Date("%Y-%m-%d").
    Date(String),
    /// Date + time: DateTime("%Y-%m-%d %H:%M").
    DateTime(String),
    /// Scientific notation: Scientific(2) → "1.23e3".
    Scientific(u8),
    /// Display as-is, no numeric interpretation.
    Text,
}

// ── Number format ──────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct NumberFormat {
    pub kind: NumberFormatKind,
}

impl NumberFormat {
    pub fn new(kind: NumberFormatKind) -> Self {
        Self { kind }
    }

    /// Format a raw cell value string for display.
    pub fn format(&self, raw: &str) -> String {
        match &self.kind {
            NumberFormatKind::General => raw.to_string(),
            NumberFormatKind::Number(dp) => format_number(raw, *dp, None),
            NumberFormatKind::Currency(sym, dp) => format_number(raw, *dp, Some(sym.as_str())),
            NumberFormatKind::Percent(dp) => format_percent(raw, *dp),
            NumberFormatKind::Date(fmt) => format_date(raw, fmt),
            NumberFormatKind::DateTime(fmt) => format_datetime(raw, fmt),
            NumberFormatKind::Scientific(dp) => format_scientific(raw, *dp),
            NumberFormatKind::Text => raw.to_string(),
        }
    }
}

impl Default for NumberFormat {
    fn default() -> Self {
        Self { kind: NumberFormatKind::General }
    }
}

// ── Formatting helpers ─────────────────────────────────────────────────

fn format_number(raw: &str, decimal_places: u8, currency: Option<&str>) -> String {
    let num = match raw.parse::<f64>() {
        Ok(n) => n,
        Err(_) => return raw.to_string(),
    };
    let int_part = num.trunc().abs() as i64;
    let frac_part = (num.abs().fract() * 10_f64.powi(decimal_places as i32)).round() as u64;
    let int_str = int_part.to_formatted_string(&Locale::en);
    let sign = if num < 0.0 { "-" } else { "" };
    let formatted = if decimal_places > 0 {
        format!("{}{}.{:0width$}", sign, int_str, frac_part, width = decimal_places as usize)
    } else {
        format!("{}{}", sign, int_str)
    };
    match currency {
        Some(sym) => format!("{}{}", sym, formatted),
        None => formatted,
    }
}

fn format_percent(raw: &str, decimal_places: u8) -> String {
    let num = match raw.parse::<f64>() {
        Ok(n) => n,
        Err(_) => return raw.to_string(),
    };
    // If value is already in percentage form (e.g., 12.3), display as-is.
    // If it's a decimal (e.g., 0.123), multiply by 100.
    let display = if num.abs() <= 1.0 && num != 0.0 {
        num * 100.0
    } else {
        num
    };
    format!("{:.*}%", decimal_places as usize, display)
}

fn format_date(raw: &str, fmt: &str) -> String {
    // Try Excel serial date first
    if let Ok(serial) = raw.parse::<f64>() {
        if let Some(date) = excel_serial_to_date(serial) {
            return date.format(fmt).to_string();
        }
    }
    // Try ISO date string
    if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return date.format(fmt).to_string();
    }
    raw.to_string()
}

fn format_datetime(raw: &str, fmt: &str) -> String {
    if let Ok(serial) = raw.parse::<f64>() {
        if let Some(dt) = excel_serial_to_datetime(serial) {
            return dt.format(fmt).to_string();
        }
    }
    raw.to_string()
}

fn format_scientific(raw: &str, decimal_places: u8) -> String {
    let num = match raw.parse::<f64>() {
        Ok(n) => n,
        Err(_) => return raw.to_string(),
    };
    format!("{:.*e}", decimal_places as usize, num)
}

// ── Excel serial date conversion ───────────────────────────────────────
//
// Excel stores dates as days since 1899-12-30 (the "1900 date system"),
// with the infamous Lotus 1-2-3 bug: 1900 is treated as a leap year.
// Serial 1 = 1899-12-31, Serial 60 = 1900-02-29 (fictional), Serial 61 = 1900-03-01.

/// Convert an Excel serial date number to a chrono NaiveDate.
/// Handles the Lotus 1-2-3 leap year bug for serials < 61.
pub fn excel_serial_to_date(serial: f64) -> Option<NaiveDate> {
    if serial <= 0.0 { return None; }
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)?;
    // For serials >= 61, subtract 1 to account for the fictional 1900-02-29.
    let adjusted = if serial >= 61.0 { serial - 1.0 } else { serial };
    epoch
        .checked_add_days(chrono::Days::new(adjusted as u64))
}

/// Convert an Excel serial date+time number to chrono NaiveDateTime.
/// Fractional part represents time (0.5 = noon).
pub fn excel_serial_to_datetime(serial: f64) -> Option<NaiveDateTime> {
    if serial <= 0.0 { return None; }
    let days = serial.floor() as i64;
    let time_fraction = serial - serial.floor();
    let seconds = (time_fraction * 86400.0).round() as i64; // 86400 secs/day
    let date = excel_serial_to_date(days as f64)?;
    date.and_hms_opt(
        (seconds / 3600) as u32,
        ((seconds % 3600) / 60) as u32,
        (seconds % 60) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_general() {
        let fmt = NumberFormat::default();
        assert_eq!(fmt.format("hello"), "hello");
        assert_eq!(fmt.format("42"), "42");
    }

    #[test]
    fn test_number() {
        let fmt = NumberFormat::new(NumberFormatKind::Number(2));
        assert_eq!(fmt.format("1234.567"), "1,234.57");
    }

    #[test]
    fn test_currency() {
        let fmt = NumberFormat::new(NumberFormatKind::Currency("$".into(), 2));
        assert_eq!(fmt.format("1234.5"), "$1,234.50");
    }

    #[test]
    fn test_percent() {
        let fmt = NumberFormat::new(NumberFormatKind::Percent(1));
        assert_eq!(fmt.format("0.123"), "12.3%");
        // Already-percentage values pass through
        assert_eq!(fmt.format("25"), "25.0%");
    }

    #[test]
    fn test_date() {
        let fmt = NumberFormat::new(NumberFormatKind::Date("%Y-%m-%d".into()));
        // Excel serial for 2026-01-01 = 46101 (after adjusting for leap year bug)
        assert_eq!(fmt.format("46101"), "2026-01-01");
        // ISO string
        assert_eq!(fmt.format("2025-06-15"), "2025-06-15");
    }

    #[test]
    fn test_excel_serial_epoch() {
        assert_eq!(
            excel_serial_to_date(1.0).unwrap().to_string(),
            "1899-12-31"
        );
        // 2026-06-24 should be around serial 46176
        let d = excel_serial_to_date(46176.0).unwrap();
        assert_eq!(d.to_string(), "2026-06-24");
    }

    #[test]
    fn test_scientific() {
        let fmt = NumberFormat::new(NumberFormatKind::Scientific(2));
        assert_eq!(fmt.format("1234"), "1.23e3");
    }
}
