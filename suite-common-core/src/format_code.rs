// SPDX-License-Identifier: GPL-3.0-or-later
//! Spreadsheet number format codes (the ECMA-376 / Excel and Calc
//! language: `#,##0.00;[Red]-#,##0.00;"–";@`), applied to a cell value.
//!
//! Supported: up to four sections (positive; negative; zero; text) and
//! `[>100]`-style conditions on the first two; digit placeholders `0`,
//! `#` and `?`; the decimal point; thousands separators and scaling
//! commas; `%`; scientific `E+00`; fractions `# ?/?` and `# ?/8`; quoted
//! literals, `\x` escapes, `_x` spacing and `*x` fill (as one character);
//! `@` for text; colours (`[Red]`), which don't change the text; and dates
//! and times: `yyyy yy mmmm mmm mm m dddd ddd dd d hh h mm ss AM/PM A/P
//! [h] [mm] [ss]` with `m` read as minutes after an hour or before
//! seconds, as Excel reads it.

use chrono::{Datelike, NaiveDateTime, Timelike};

/// One piece of a section, in order.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Lit(String),
    Digit(char), // '0', '#' or '?'
    Point,
    Comma,
    Percent,
    Exp(bool),   // E+ (true) or E-
    Slash,
    Text,        // @
    General,
    Date(String), // a date/time code: "yyyy", "mm", "h", "AM/PM", "[h]"…
}

#[derive(Debug, Clone, Default)]
struct Section {
    toks: Vec<Tok>,
    cond: Option<(String, f64)>,
}

fn split_sections(code: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let (mut quoted, mut bracket, mut escaped) = (false, false, false);
    for ch in code.chars() {
        let last = out.last_mut().unwrap();
        if escaped {
            last.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if !quoted => {
                escaped = true;
                last.push(ch);
            }
            '"' => {
                quoted = !quoted;
                last.push(ch);
            }
            '[' if !quoted => {
                bracket = true;
                last.push(ch);
            }
            ']' if !quoted => {
                bracket = false;
                last.push(ch);
            }
            ';' if !quoted && !bracket => out.push(String::new()),
            _ => last.push(ch),
        }
    }
    out
}

fn parse_section(src: &str) -> Section {
    let chars: Vec<char> = src.chars().collect();
    let mut s = Section::default();
    let mut i = 0;
    let lower = |c: char| c.to_ascii_lowercase();
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                let end = chars[i + 1..].iter().position(|&x| x == '"').map_or(chars.len(), |p| i + 1 + p);
                s.toks.push(Tok::Lit(chars[i + 1..end].iter().collect()));
                i = end + 1;
            }
            '\\' => {
                if let Some(&n) = chars.get(i + 1) {
                    s.toks.push(Tok::Lit(n.to_string()));
                }
                i += 2;
            }
            '_' => {
                // Space as wide as the next character: one space.
                s.toks.push(Tok::Lit(" ".into()));
                i += 2;
            }
            '*' => {
                // Repeat to fill the cell: drawn once.
                if let Some(&n) = chars.get(i + 1) {
                    s.toks.push(Tok::Lit(n.to_string()));
                }
                i += 2;
            }
            '[' => {
                let end = chars[i..].iter().position(|&x| x == ']').map_or(chars.len(), |p| i + p);
                let inner: String = chars[i + 1..end].iter().collect();
                let lo = inner.to_ascii_lowercase();
                if lo == "h" || lo == "hh" || lo == "m" || lo == "mm" || lo == "s" || lo == "ss" {
                    s.toks.push(Tok::Date(format!("[{lo}]")));
                } else if let Some(op) = ["<=", ">=", "<>", "<", ">", "="].iter().find(|op| inner.starts_with(**op)) {
                    if let Ok(v) = inner[op.len()..].trim().parse() {
                        s.cond = Some((op.to_string(), v));
                    }
                }
                // Colours and locale codes ([$€-407]) change nothing drawn
                // here, but a currency symbol in them is text.
                if let Some(sym) = inner.strip_prefix('$') {
                    let sym = sym.split('-').next().unwrap_or("");
                    if !sym.is_empty() {
                        s.toks.push(Tok::Lit(sym.into()));
                    }
                }
                i = end + 1;
            }
            '0' | '#' | '?' => {
                s.toks.push(Tok::Digit(c));
                i += 1;
            }
            '.' => {
                s.toks.push(Tok::Point);
                i += 1;
            }
            ',' => {
                s.toks.push(Tok::Comma);
                i += 1;
            }
            '%' => {
                s.toks.push(Tok::Percent);
                i += 1;
            }
            '/' => {
                s.toks.push(Tok::Slash);
                i += 1;
            }
            '@' => {
                s.toks.push(Tok::Text);
                i += 1;
            }
            'E' | 'e' if matches!(chars.get(i + 1), Some('+') | Some('-')) => {
                s.toks.push(Tok::Exp(chars[i + 1] == '+'));
                i += 2;
            }
            _ if src[src.char_indices().nth(i).map_or(0, |x| x.0)..].to_ascii_lowercase().starts_with("general") => {
                s.toks.push(Tok::General);
                i += 7;
            }
            _ if chars[i..].iter().take(5).collect::<String>().eq_ignore_ascii_case("AM/PM") => {
                s.toks.push(Tok::Date("AM/PM".into()));
                i += 5;
            }
            _ if chars[i..].iter().take(3).collect::<String>().eq_ignore_ascii_case("A/P") => {
                s.toks.push(Tok::Date("A/P".into()));
                i += 3;
            }
            _ if matches!(lower(c), 'y' | 'm' | 'd' | 'h' | 's') => {
                let run = chars[i..].iter().take_while(|&&x| lower(x) == lower(c)).count();
                s.toks.push(Tok::Date(std::iter::repeat_n(lower(c), run).collect()));
                i += run;
            }
            _ => {
                s.toks.push(Tok::Lit(c.to_string()));
                i += 1;
            }
        }
    }
    // `m` is minutes after an hour or before seconds.
    let dates: Vec<usize> = (0..s.toks.len()).filter(|&k| matches!(s.toks[k], Tok::Date(_))).collect();
    for (n, &k) in dates.iter().enumerate() {
        let Tok::Date(code) = &s.toks[k] else { continue };
        if code != "m" && code != "mm" {
            continue;
        }
        let prev = n.checked_sub(1).and_then(|p| match &s.toks[dates[p]] {
            Tok::Date(c) => Some(c.clone()),
            _ => None,
        });
        let next = dates.get(n + 1).and_then(|&q| match &s.toks[q] {
            Tok::Date(c) => Some(c.clone()),
            _ => None,
        });
        let after_hour = prev.is_some_and(|p| p.starts_with('h') || p.starts_with("[h"));
        let before_sec = next.is_some_and(|p| p.starts_with('s') || p.starts_with("[s"));
        if after_hour || before_sec {
            s.toks[k] = Tok::Date(if code == "m" { "M".into() } else { "MM".into() });
        }
    }
    s
}

fn is_date(s: &Section) -> bool {
    s.toks.iter().any(|t| matches!(t, Tok::Date(_)))
}

fn cond_holds((op, v): &(String, f64), x: f64) -> bool {
    match op.as_str() {
        "<" => x < *v,
        "<=" => x <= *v,
        ">" => x > *v,
        ">=" => x >= *v,
        "=" => x == *v,
        "<>" => x != *v,
        _ => false,
    }
}

/// `value` formatted with `code`. A value that isn't a number goes through
/// the text section (or shows as it is).
pub fn format_with_code(code: &str, value: &str) -> String {
    let sections: Vec<Section> = split_sections(code).iter().map(|s| parse_section(s)).collect();
    let Ok(x) = value.trim().parse::<f64>() else {
        let text_section = sections.get(3).or_else(|| sections.iter().find(|s| s.toks.contains(&Tok::Text)));
        return match text_section {
            Some(s) => s
                .toks
                .iter()
                .map(|t| match t {
                    Tok::Text => value.to_string(),
                    Tok::Lit(l) => l.clone(),
                    _ => String::new(),
                })
                .collect(),
            None => value.to_string(),
        };
    };
    // Which section, and whether it shows the sign itself.
    let conditional = sections.first().is_some_and(|s| s.cond.is_some());
    let (section, abs) = if conditional {
        if sections[0].cond.as_ref().is_some_and(|c| cond_holds(c, x)) {
            (&sections[0], false)
        } else if sections.get(1).is_some_and(|s| s.cond.as_ref().is_none_or(|c| cond_holds(c, x))) {
            (&sections[1], sections[1].cond.is_none() && x < 0.0)
        } else {
            (sections.get(2).unwrap_or(&sections[0]), false)
        }
    } else {
        match sections.len() {
            1 => (&sections[0], false),
            _ if x > 0.0 => (&sections[0], false),
            _ if x < 0.0 => (&sections[1], true),
            _ => (sections.get(2).unwrap_or(&sections[0]), false),
        }
    };
    let x = if abs { x.abs() } else { x };
    if is_date(section) {
        return format_date(section, x);
    }
    format_number(section, x)
}

fn general(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        let s = format!("{:.10}", x);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn format_number(s: &Section, x: f64) -> String {
    let toks = &s.toks;
    if toks.iter().all(|t| matches!(t, Tok::Lit(_) | Tok::General)) && toks.contains(&Tok::General) {
        let sign = if x < 0.0 { "-" } else { "" };
        return toks
            .iter()
            .map(|t| match t {
                Tok::General => format!("{sign}{}", general(x.abs())),
                Tok::Lit(l) => l.clone(),
                _ => String::new(),
            })
            .collect();
    }
    let digit_count = toks.iter().filter(|t| matches!(t, Tok::Digit(_))).count();
    if digit_count == 0 && toks.contains(&Tok::Text) {
        // A text format shows a number as it is.
        return general(x);
    }
    if digit_count == 0 {
        // Only literals: the section's text, with the sign for negatives
        // in a one-section code.
        return toks.iter().map(|t| if let Tok::Lit(l) = t { l.clone() } else { String::new() }).collect();
    }
    if toks.contains(&Tok::Slash) {
        return format_fraction(s, x);
    }
    let mut v = x;
    let percents = toks.iter().filter(|t| **t == Tok::Percent).count();
    v *= 100f64.powi(percents as i32);
    // Commas right after the last digit placeholder (before the point or
    // the end) scale by a thousand each; a comma between integer
    // placeholders groups thousands.
    let last_digit = toks.iter().rposition(|t| matches!(t, Tok::Digit(_))).unwrap();
    let point = toks.iter().position(|t| *t == Tok::Point);
    let int_end = point.unwrap_or(last_digit + 1);
    // Scaling commas follow the last digit placeholder, or stand just
    // before the decimal point.
    let trailing = toks[last_digit + 1..].iter().take_while(|t| **t == Tok::Comma).count();
    let before_point = point.map_or(0, |p| toks[..p].iter().rev().take_while(|t| **t == Tok::Comma).count());
    let scale = if point.is_some_and(|p| p > last_digit) { before_point } else { trailing + before_point };
    v /= 1000f64.powi(scale as i32);
    let first_digit = toks.iter().position(|t| matches!(t, Tok::Digit(_))).unwrap();
    let grouping = toks[first_digit..int_end.min(toks.len())].iter().enumerate().any(|(n, t)| {
        *t == Tok::Comma && toks[first_digit + n + 1..int_end].iter().any(|t| matches!(t, Tok::Digit(_)))
    });

    let exp = toks.iter().position(|t| matches!(t, Tok::Exp(_)));
    let int_places: Vec<char> = toks[..exp.unwrap_or(int_end).min(int_end)]
        .iter()
        .filter_map(|t| if let Tok::Digit(d) = t { Some(*d) } else { None })
        .collect();
    let frac_places: Vec<char> = match point {
        Some(p) => toks[p + 1..exp.unwrap_or(toks.len())].iter().filter_map(|t| if let Tok::Digit(d) = t { Some(*d) } else { None }).collect(),
        None => Vec::new(),
    };
    let mut exponent = 0i32;
    if exp.is_some() && v != 0.0 {
        let int_digits = int_places.len().max(1) as i32;
        exponent = (v.abs().log10().floor() as i32) - (int_digits - 1);
        v /= 10f64.powi(exponent);
        // Rounding can push the mantissa up a digit.
        let rounded = (v.abs() * 10f64.powi(frac_places.len() as i32)).round() / 10f64.powi(frac_places.len() as i32);
        if rounded >= 10f64.powi(int_digits) {
            v /= 10.0;
            exponent += 1;
        }
    }
    let negative = v < 0.0;
    let scaled = (v.abs() * 10f64.powi(frac_places.len() as i32)).round();
    let int_value = (scaled / 10f64.powi(frac_places.len() as i32)).trunc();
    let frac_value = scaled - int_value * 10f64.powi(frac_places.len() as i32);
    let mut int_digits: Vec<char> = if int_value == 0.0 { Vec::new() } else { format!("{:.0}", int_value).chars().collect() };
    let mut frac_digits: Vec<char> = format!("{:0width$.0}", frac_value, width = frac_places.len()).chars().collect();
    if frac_places.is_empty() {
        frac_digits.clear();
    }
    // Minimum integer digits: the 0s (a ? pads with a space).
    let min_int = int_places.iter().filter(|c| **c == '0' || **c == '?').count();
    while int_digits.len() < min_int {
        let pad = int_places[int_places.len() - int_digits.len() - 1];
        int_digits.insert(0, if pad == '?' { ' ' } else { '0' });
    }
    if grouping {
        let mut grouped = Vec::new();
        let digits: Vec<char> = int_digits.clone();
        for (n, c) in digits.iter().enumerate() {
            if n > 0 && (digits.len() - n).is_multiple_of(3) && c.is_ascii_digit() && digits[n - 1].is_ascii_digit() {
                grouped.push(',');
            }
            grouped.push(*c);
        }
        int_digits = grouped;
    }
    // Trailing optional decimals: # drops a zero, ? becomes a space.
    for (n, place) in frac_places.iter().enumerate().rev() {
        if frac_digits.get(n) != Some(&'0') || *place == '0' {
            break;
        }
        frac_digits[n] = if *place == '?' { ' ' } else { '\0' };
    }
    let frac: String = frac_digits.into_iter().filter(|c| *c != '\0').collect();

    // Lay the section out: integer digits go where the first integer
    // placeholder is, the fraction after the point, literals as written.
    let mut out = String::new();
    if negative && x < 0.0 {
        out.push('-');
    }
    let mut placed_int = false;
    let mut in_exp = false;
    let mut exp_digits_done = false;
    for (n, t) in toks.iter().enumerate() {
        match t {
            Tok::Lit(l) => out.push_str(l),
            Tok::Percent => out.push('%'),
            Tok::Digit(_) if in_exp => {
                if !exp_digits_done {
                    let width = toks[n..].iter().filter(|t| matches!(t, Tok::Digit(_))).count();
                    out.push_str(&format!("{:0width$}", exponent.abs()));
                    exp_digits_done = true;
                }
            }
            Tok::Digit(_) if n < int_end && exp.is_none_or(|e| n < e) => {
                if !placed_int {
                    out.extend(int_digits.iter());
                    placed_int = true;
                }
            }
            Tok::Digit(_) => {}
            Tok::Point => {
                if !placed_int {
                    out.extend(int_digits.iter());
                    placed_int = true;
                }
                if !frac.is_empty() || frac_places.contains(&'?') {
                    out.push('.');
                    out.push_str(&frac);
                }
            }
            Tok::Exp(plus) => {
                out.push('E');
                if exponent < 0 {
                    out.push('-');
                } else if *plus {
                    out.push('+');
                }
                in_exp = true;
            }
            Tok::Comma | Tok::Slash | Tok::Text | Tok::General | Tok::Date(_) => {}
        }
    }
    out
}

/// `# ?/?`, `# ??/??`, `?/8` and the like.
fn format_fraction(s: &Section, x: f64) -> String {
    let toks = &s.toks;
    let slash = toks.iter().position(|t| *t == Tok::Slash).unwrap();
    // A fixed denominator, if written as digits after the slash.
    let den_text: String = toks[slash + 1..].iter().map(|t| match t {
        Tok::Digit(d) => d.to_string(),
        Tok::Lit(l) if l.chars().all(|c| c.is_ascii_digit()) => l.clone(),
        _ => String::new(),
    }).collect();
    let den_places = toks[slash + 1..].iter().filter(|t| matches!(t, Tok::Digit('?') | Tok::Digit('#'))).count();
    // A whole-number part when there are placeholders before a space.
    let has_whole = toks[..slash].iter().any(|t| matches!(t, Tok::Lit(l) if l == " "));
    let sign = if x < 0.0 { "-" } else { "" };
    let a = x.abs();
    let (whole, frac) = if has_whole { (a.trunc(), a.fract()) } else { (0.0, a) };
    let (num, den) = if let Ok(d) = den_text.parse::<u64>().map_err(|_| ()).and_then(|d| if den_places == 0 && d > 0 { Ok(d) } else { Err(()) }) {
        ((frac * d as f64).round() as u64, d)
    } else {
        let max_den = 10u64.pow(den_places.clamp(1, 4) as u32) - 1;
        let (mut best_n, mut best_d, mut best_err) = (0u64, 1u64, f64::INFINITY);
        for d in 1..=max_den {
            let n = (frac * d as f64).round() as u64;
            let err = (frac - n as f64 / d as f64).abs();
            if err < best_err - 1e-12 {
                (best_n, best_d, best_err) = (n, d, err);
            }
        }
        (best_n, best_d)
    };
    let (whole, num) = if num == den && has_whole { (whole + 1.0, 0) } else { (whole, num) };
    let prefix: String = toks.iter().take_while(|t| matches!(t, Tok::Lit(_))).map(|t| if let Tok::Lit(l) = t { l.clone() } else { String::new() }).collect();
    match (has_whole, num) {
        (true, 0) => format!("{prefix}{sign}{}", whole as i64),
        (true, n) if whole == 0.0 => format!("{prefix}{sign}{n}/{den}"),
        (true, n) => format!("{prefix}{sign}{} {n}/{den}", whole as i64),
        (false, n) => format!("{prefix}{sign}{n}/{den}"),
    }
}

fn format_date(s: &Section, serial: f64) -> String {
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).unwrap().and_hms_opt(0, 0, 0).unwrap();
    // Excel's 1900 is a leap year (the Lotus bug): serials 1 to 60 are a
    // day later than a plain count from 1899-12-30 gives.
    let shift = if (1.0..61.0).contains(&serial) { 86_400_000 } else { 0 };
    let millis = (serial * 86_400_000.0).round() as i64 + shift;
    let Some(dt): Option<NaiveDateTime> = epoch.checked_add_signed(chrono::Duration::milliseconds(millis)) else {
        return general(serial);
    };
    let twelve = s.toks.iter().any(|t| matches!(t, Tok::Date(c) if c == "AM/PM" || c == "A/P"));
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let hour = if twelve { (dt.hour() + 11) % 12 + 1 } else { dt.hour() };
    let total_hours = (serial * 24.0).floor() as i64;
    let mut out = String::new();
    for t in &s.toks {
        match t {
            Tok::Lit(l) => out.push_str(l),
            Tok::Date(c) => out.push_str(&match c.as_str() {
                "yyyy" | "yyy" => format!("{:04}", dt.year()),
                "yy" | "y" => format!("{:02}", dt.year() % 100),
                "mmmmm" => MONTHS[dt.month0() as usize][..1].to_string(),
                "mmmm" => MONTHS[dt.month0() as usize].to_string(),
                "mmm" => MONTHS[dt.month0() as usize][..3].to_string(),
                "mm" => format!("{:02}", dt.month()),
                "m" => dt.month().to_string(),
                "dddd" => DAYS[dt.weekday().num_days_from_monday() as usize].to_string(),
                "ddd" => DAYS[dt.weekday().num_days_from_monday() as usize][..3].to_string(),
                "dd" => format!("{:02}", dt.day()),
                "d" => dt.day().to_string(),
                "hh" => format!("{:02}", hour),
                "h" => hour.to_string(),
                "MM" => format!("{:02}", dt.minute()),
                "M" => dt.minute().to_string(),
                "ss" => format!("{:02}", dt.second()),
                "s" => dt.second().to_string(),
                "[h]" | "[hh]" => total_hours.to_string(),
                "[m]" | "[mm]" => ((serial * 1440.0).floor() as i64).to_string(),
                "[s]" | "[ss]" => ((serial * 86400.0).round() as i64).to_string(),
                "AM/PM" => if dt.hour() < 12 { "AM" } else { "PM" }.to_string(),
                "A/P" => if dt.hour() < 12 { "A" } else { "P" }.to_string(),
                other => other.to_string(),
            }),
            Tok::Point => out.push('.'),
            Tok::Digit(d) => out.push(*d),
            Tok::Comma => out.push(','),
            Tok::Percent => out.push('%'),
            Tok::Slash => out.push('/'),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format_with_code as f;

    #[test]
    fn numbers_as_excel_shows_them() {
        assert_eq!(f("0.00", "3.14159"), "3.14");
        assert_eq!(f("#,##0", "1234567.8"), "1,234,568");
        assert_eq!(f("#,##0.00", "-1234.5"), "-1,234.50");
        assert_eq!(f("000", "7"), "007");
        assert_eq!(f("#.##", "2.5"), "2.5");
        assert_eq!(f("0.0#", "2"), "2.0");
        assert_eq!(f("#", "0"), "");
        assert_eq!(f("0%", "0.256"), "26%");
        assert_eq!(f("0.0%", "0.256"), "25.6%");
        assert_eq!(f("#,##0,", "1234567"), "1,235");
        assert_eq!(f("0.0,,\"M\"", "12345678"), "12.3M");
        assert_eq!(f("0.00E+00", "12345"), "1.23E+04");
        assert_eq!(f("0.00E+00", "0.00012"), "1.20E-04");
        assert_eq!(f("\"$\"#,##0.00", "1234.5"), "$1,234.50");
        assert_eq!(f("[$€-407]#,##0.00", "9.5"), "€9.50");
        assert_eq!(f("0.00\\ \"kg\"", "3"), "3.00 kg");
    }

    #[test]
    fn sections_choose_by_sign_and_text() {
        let code = "#,##0.00;[Red](#,##0.00);\"–\";\"Note: \"@";
        assert_eq!(f(code, "1500"), "1,500.00");
        assert_eq!(f(code, "-1500"), "(1,500.00)");
        assert_eq!(f(code, "0"), "–");
        assert_eq!(f(code, "late"), "Note: late");
        // Two sections: the second draws negatives, sign and all.
        assert_eq!(f("0;-0.0", "-2"), "-2.0");
    }

    #[test]
    fn conditions_pick_the_section() {
        let code = "[>=1000]#,##0,\"K\";0";
        assert_eq!(f(code, "25000"), "25K");
        assert_eq!(f(code, "25"), "25");
    }

    #[test]
    fn fractions() {
        assert_eq!(f("# ?/?", "1.5"), "1 1/2");
        assert_eq!(f("# ??/??", "0.3333"), "1/3");
        assert_eq!(f("# ?/8", "2.37"), "2 3/8");
    }

    #[test]
    fn dates_and_times() {
        // 45306.75 is 2024-01-15 18:00.
        assert_eq!(f("yyyy-mm-dd", "45306.75"), "2024-01-15");
        assert_eq!(f("d mmmm yyyy", "45306"), "15 January 2024");
        assert_eq!(f("ddd, mmm d", "45306"), "Mon, Jan 15");
        assert_eq!(f("h:mm AM/PM", "45306.75"), "6:00 PM");
        assert_eq!(f("hh:mm:ss", "0.5"), "12:00:00");
        assert_eq!(f("[h]:mm", "1.5"), "36:00");
        assert_eq!(f("m/d/yy", "45306"), "1/15/24");
    }

    #[test]
    fn text_and_general() {
        assert_eq!(f("@", "abc"), "abc");
        assert_eq!(f("General", "0.5"), "0.5");
        assert_eq!(f("\"Total: \"General", "12"), "Total: 12");
        assert_eq!(f("0.00", "n/a"), "n/a");
    }
}
