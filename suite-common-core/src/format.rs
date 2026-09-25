// format.rs — Shared number formatting engine for suite-common.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice svl/ SvNumberFormatter + SvNumberformatInfo.
// Formats numbers/dates/currencies/percentages for display in Tables cells,
// Letters fields, and Decks text boxes.

use chrono::{NaiveDate, NaiveDateTime};
use num_format::{Locale, ToFormattedString};

// ── Format kind ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// Mixed fraction with denominators of up to this many digits:
    /// Fraction(1) → "1 1/2", as Excel's `# ?/?`.
    Fraction(u8),
    /// Any other format code, as Excel and Calc write it
    /// (`#,##0.00;[Red](#,##0.00)`), applied by [`crate::format_code`].
    Custom(String),
}

// ── Number format ──────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
            NumberFormatKind::Fraction(digits) => format_fraction(raw, *digits),
            NumberFormatKind::Custom(code) => crate::format_code::format_with_code(code, raw),
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
    // Round the whole value once, as Excel does: 45306.75 to no places is
    // 45,307, and 0.999 to two is 1.00. Truncating the integer and rounding
    // the fraction apart lost the carry.
    let scale = 10_f64.powi(decimal_places as i32);
    let scaled = (num.abs() * scale).round();
    let int_part = (scaled / scale).trunc() as i64;
    let frac_part = (scaled - int_part as f64 * scale).round() as u64;
    let int_str = int_part.to_formatted_string(&Locale::en);
    let sign = if num < 0.0 && scaled > 0.0 { "-" } else { "" };
    let digits = if decimal_places > 0 {
        format!("{}.{:0width$}", int_str, frac_part, width = decimal_places as usize)
    } else {
        int_str
    };
    // The sign goes before the currency symbol: -$1,234.50.
    format!("{sign}{}{digits}", currency.unwrap_or(""))
}

fn format_percent(raw: &str, decimal_places: u8) -> String {
    let num = match raw.parse::<f64>() {
        Ok(n) => n,
        Err(_) => return raw.to_string(),
    };
    // A percentage shows the value times a hundred, as Excel and Calc do:
    // 0.123 is 12.3% and 12 is 1200%. Treating values over 1 as already
    // being percentages drew 12 as 12%, which no spreadsheet does.
    format!("{:.*}%", decimal_places as usize, num * 100.0)
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

/// `value` as a mixed fraction whose denominator has at most `digits`
/// digits, choosing the closest such fraction (Excel's `# ?/?` family).
fn format_fraction(raw: &str, digits: u8) -> String {
    let Ok(num) = raw.parse::<f64>() else { return raw.to_string() };
    let max_den = 10u64.saturating_pow(digits.clamp(1, 4) as u32) - 1;
    let sign = if num < 0.0 { "-" } else { "" };
    let a = num.abs();
    let whole = a.trunc() as u64;
    let frac = a - whole as f64;
    let (mut best_n, mut best_d, mut best_err) = (0u64, 1u64, f64::INFINITY);
    for d in 1..=max_den {
        let n = (frac * d as f64).round() as u64;
        let err = (frac - n as f64 / d as f64).abs();
        if err < best_err - 1e-12 {
            (best_n, best_d, best_err) = (n, d, err);
        }
    }
    let (whole, best_n) = if best_n == best_d { (whole + 1, 0) } else { (whole, best_n) };
    match (whole, best_n) {
        (w, 0) => format!("{sign}{w}"),
        (0, n) => format!("{sign}{n}/{best_d}"),
        (w, n) => format!("{sign}{w} {n}/{best_d}"),
    }
}

fn format_scientific(raw: &str, decimal_places: u8) -> String {
    let num = match raw.parse::<f64>() {
        Ok(n) => n,
        Err(_) => return raw.to_string(),
    };
    // As Excel and Calc write it: 1.23E+04, 1.20E-04.
    let s = format!("{:.*e}", decimal_places as usize, num);
    let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    format!("{mantissa}E{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
}

// ── Excel serial date conversion ───────────────────────────────────────
//
// Excel stores dates as days since 1899-12-30 (the "1900 date system"),
// with the infamous Lotus 1-2-3 bug: 1900 is treated as a leap year.
// Serial 1 = 1899-12-31, Serial 60 = 1900-02-29 (fictional), Serial 61 = 1900-03-01.

/// Convert an Excel serial date number to a chrono NaiveDate.
///
/// Serial 1 is 1900-01-01. Serial 60 is Excel's fictional 1900-02-29 (the
/// Lotus 1-2-3 bug), which has no real date; it maps to 1900-02-28. From
/// serial 61 (1900-03-01) on, day N is simply 1899-12-30 + N: that epoch
/// already absorbs the fictional day. This used to subtract one more day,
/// so every modern date showed a day early (2023-03-15 as 2023-03-14).
pub fn excel_serial_to_date(serial: f64) -> Option<NaiveDate> {
    if serial < 1.0 { return None; }
    let days = serial.floor() as u64;
    let (epoch, days) = match days {
        1..=59 => (NaiveDate::from_ymd_opt(1899, 12, 31)?, days),
        60 => return NaiveDate::from_ymd_opt(1900, 2, 28),
        _ => (NaiveDate::from_ymd_opt(1899, 12, 30)?, days),
    };
    epoch.checked_add_days(chrono::Days::new(days))
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
    fn fractions_pick_the_closest_denominator_in_range() {
        let f = |d, v: &str| NumberFormat::new(NumberFormatKind::Fraction(d)).format(v);
        assert_eq!(f(1, "0.5"), "1/2");
        assert_eq!(f(1, "1.25"), "1 1/4");
        assert_eq!(f(1, "-0.75"), "-3/4");
        assert_eq!(f(1, "3"), "3");
        assert_eq!(f(1, "0.999"), "1");
        assert_eq!(f(2, "0.3333"), "1/3");
        assert_eq!(f(1, "abc"), "abc");
    }
    use chrono::Datelike;

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
        // A value over 1 is multiplied too, as in Excel and Calc.
        assert_eq!(fmt.format("25"), "2500.0%");
    }

    #[test]
    fn test_date() {
        let fmt = NumberFormat::new(NumberFormatKind::Date("%Y-%m-%d".into()));
        // ISO string passthrough
        assert_eq!(fmt.format("2025-06-15"), "2025-06-15");
        // Excel serial 45000 is 2023-03-15 (what Excel and LibreOffice show).
        assert_eq!(fmt.format("45000"), "2023-03-15");
    }

    #[test]
    fn test_excel_serial_epoch() {
        let ymd = |s: f64| excel_serial_to_date(s).map(|d| (d.year(), d.month(), d.day()));
        assert_eq!(ymd(1.0), Some((1900, 1, 1)));
        assert_eq!(ymd(59.0), Some((1900, 2, 28)));
        assert_eq!(ymd(60.0), Some((1900, 2, 28)), "the fictional 1900-02-29");
        assert_eq!(ymd(61.0), Some((1900, 3, 1)));
        assert_eq!(ymd(45292.0), Some((2024, 1, 1)));
        assert_eq!(ymd(45000.75), Some((2023, 3, 15)), "the time of day doesn't move the date");
        assert_eq!(ymd(0.0), None);
    }

    #[test]
    fn test_scientific() {
        let fmt = NumberFormat::new(NumberFormatKind::Scientific(2));
        assert_eq!(fmt.format("1234"), "1.23E+03");
    }

    #[test]
    fn test_datetime() {
        let fmt = NumberFormat::new(NumberFormatKind::DateTime("%Y-%m-%d %H:%M".into()));
        // Non-numeric strings pass through
        assert_eq!(fmt.format("hello"), "hello");
    }

    #[test]
    fn test_text() {
        let fmt = NumberFormat::new(NumberFormatKind::Text);
        assert_eq!(fmt.format("1234"), "1234");
        assert_eq!(fmt.format("hello"), "hello");
    }

    #[test]
    fn test_number_zero() {
        let fmt = NumberFormat::new(NumberFormatKind::Number(2));
        assert_eq!(fmt.format("0"), "0.00");
    }

    #[test]
    fn test_number_negative() {
        let fmt = NumberFormat::new(NumberFormatKind::Number(2));
        assert_eq!(fmt.format("-50"), "-50.00");
    }

    #[test]
    fn test_currency_zero() {
        let fmt = NumberFormat::new(NumberFormatKind::Currency("$".into(), 2));
        assert_eq!(fmt.format("0"), "$0.00");
    }

    #[test]
    fn test_percent_whole() {
        let fmt = NumberFormat::new(NumberFormatKind::Percent(0));
        assert_eq!(fmt.format("0.5"), "50%");
    }

    #[test]
    fn test_invalid_number_passthrough() {
        let fmt = NumberFormat::new(NumberFormatKind::Number(2));
        assert_eq!(fmt.format("notanumber"), "notanumber");
    }
}
