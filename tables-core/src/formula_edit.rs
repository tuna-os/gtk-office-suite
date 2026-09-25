// formula_edit.rs — what the formula bar knows about the formula being typed.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// docs/DESIGN-UI.md, "Formula editor with range tokens" and "formula
// autocomplete with argument hints" (Numbers and Sheets): references in
// the formula show as coloured tokens, each the colour of the outline the
// grid draws round its range; typing a function name offers the functions
// it could be; inside a call, the signature shows with the current
// argument picked out. All of it is text in, spans and hints out, so it is
// tested here without GTK.

use crate::sheet::parse_cell_ref;

/// The reference palette, in order of first appearance: the same colours
/// Excel and Sheets cycle through. The grid's outlines and the formula
/// bar's tokens both take reference `i`'s colour from here.
pub const REF_COLORS: [(u8, u8, u8); 5] = [
    (0x21, 0x61, 0xC4), // blue
    (0xC4, 0x21, 0x21), // red
    (0x21, 0x99, 0x33), // green
    (0x8C, 0x33, 0xB2), // purple
    (0xD9, 0x80, 0x00), // orange
];

/// One reference in formula text.
#[derive(Clone, Debug, PartialEq)]
pub struct RefToken {
    /// Byte range of the reference in the text, sheet prefix included.
    pub span: (usize, usize),
    /// (top, left, bottom, right), 0-based inclusive.
    pub rect: (usize, usize, usize, usize),
    /// Index into REF_COLORS (modulo its length): references to the same
    /// range share a colour, as in Sheets.
    pub color: usize,
}

/// The references in `text` (a formula, leading `=` optional) that point
/// at this sheet: explicitly named `sheet_name!` or unqualified, and not a
/// function name (`LOG10(`) or one of `defined_names` that merely looks
/// like a cell reference.
pub fn reference_tokens(text: &str, sheet_name: &str, defined_names: &[String]) -> Vec<RefToken> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?:('(?:[^']|'')+'|[A-Za-z_][A-Za-z0-9_.]*)!)?(\$?[A-Za-z]{1,3}\$?[0-9]+)(?::(\$?[A-Za-z]{1,3}\$?[0-9]+))?",
        )
        .unwrap()
    });
    let strings = string_spans(text);
    let mut out: Vec<RefToken> = Vec::new();
    let mut rects: Vec<(usize, usize, usize, usize)> = Vec::new();
    for cap in re.captures_iter(text) {
        let whole = cap.get(0).unwrap();
        // Inside a string literal, part of a longer name, or a call.
        if strings.iter().any(|&(a, b)| whole.start() >= a && whole.start() < b)
            || text[..whole.start()].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
            || text[whole.end()..].starts_with('(')
            || text[whole.end()..].chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        if let Some(sheet) = cap.get(1) {
            let name = sheet.as_str().trim_matches('\'').replace("''", "'");
            if !name.eq_ignore_ascii_case(sheet_name) {
                continue;
            }
        }
        let first = cap.get(2).unwrap().as_str().replace('$', "");
        if cap.get(3).is_none() && defined_names.iter().any(|n| n.eq_ignore_ascii_case(&first)) {
            continue;
        }
        let Some((r0, c0)) = parse_cell_ref(&first) else { continue };
        let (r1, c1) = match cap.get(3) {
            Some(m) => match parse_cell_ref(&m.as_str().replace('$', "")) {
                Some(rc) => rc,
                None => continue,
            },
            None => (r0, c0),
        };
        let rect = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
        let color = rects.iter().position(|r| *r == rect).unwrap_or_else(|| {
            rects.push(rect);
            rects.len() - 1
        });
        out.push(RefToken { span: (whole.start(), whole.end()), rect, color });
    }
    out
}

/// The distinct ranges of `tokens`, in colour order: what the grid
/// outlines, so outline `i` is drawn in `REF_COLORS[i]`.
pub fn distinct_ranges(tokens: &[RefToken]) -> Vec<(usize, usize, usize, usize)> {
    let mut out: Vec<(usize, usize, usize, usize)> = Vec::new();
    for t in tokens {
        if t.color == out.len() {
            out.push(t.rect);
        }
    }
    out
}

/// Byte ranges of the string literals ("…", with "" as an escaped quote).
fn string_spans(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            match start {
                None => start = Some(i),
                Some(_) if bytes.get(i + 1) == Some(&b'"') => i += 1,
                Some(s) => {
                    out.push((s, i + 1));
                    start = None;
                }
            }
        }
        i += 1;
    }
    // An unclosed string runs past the end: a cursor at the end is in it.
    if let Some(s) = start {
        out.push((s, bytes.len() + 1));
    }
    out
}

/// A function the formula bar offers: its name, its arguments (optional
/// ones in brackets, a repeatable one ending "…") and one line on what it
/// does.
#[derive(Debug, PartialEq)]
pub struct FunctionInfo {
    pub name: &'static str,
    pub args: &'static [&'static str],
    pub summary: &'static str,
}

macro_rules! f {
    ($name:literal, [$($arg:literal),*], $summary:literal) => {
        FunctionInfo { name: $name, args: &[$($arg),*], summary: $summary }
    };
}

/// The functions offered, alphabetical. Each is one the engine evaluates
/// (tested: `every_catalogued_function_is_one_the_engine_knows`), so a
/// suggestion never leads to #NAME?.
pub static FUNCTIONS: &[FunctionInfo] = &[
    f!("ABS", ["number"], "Absolute value"),
    f!("AND", ["logical1", "[logical2]…"], "TRUE if every argument is TRUE"),
    f!("AVERAGE", ["number1", "[number2]…"], "Arithmetic mean"),
    f!("AVERAGEIF", ["range", "criteria", "[average_range]"], "Mean of the cells that meet a condition"),
    f!("CONCAT", ["text1", "[text2]…"], "Joins text"),
    f!("COUNT", ["value1", "[value2]…"], "Counts the numbers"),
    f!("COUNTA", ["value1", "[value2]…"], "Counts the non-empty cells"),
    f!("COUNTBLANK", ["range"], "Counts the empty cells"),
    f!("COUNTIF", ["range", "criteria"], "Counts the cells that meet a condition"),
    f!("DATE", ["year", "month", "day"], "A date from its parts"),
    f!("DAY", ["date"], "Day of the month"),
    f!("HLOOKUP", ["lookup_value", "table", "row_index", "[range_lookup]"], "Looks up a value in the top row"),
    f!("IF", ["logical_test", "value_if_true", "[value_if_false]"], "One value if a condition is true, another if not"),
    f!("IFERROR", ["value", "value_if_error"], "A fallback when a value is an error"),
    f!("INDEX", ["array", "row_num", "[column_num]"], "The value at a position in a range"),
    f!("INT", ["number"], "Rounds down to an integer"),
    f!("LEFT", ["text", "[num_chars]"], "First characters of text"),
    f!("LEN", ["text"], "Length of text"),
    f!("LOWER", ["text"], "Text in lower case"),
    f!("MATCH", ["lookup_value", "lookup_array", "[match_type]"], "Position of a value in a range"),
    f!("MAX", ["number1", "[number2]…"], "Largest value"),
    f!("MEDIAN", ["number1", "[number2]…"], "Middle value"),
    f!("MID", ["text", "start_num", "num_chars"], "Characters from the middle of text"),
    f!("MIN", ["number1", "[number2]…"], "Smallest value"),
    f!("MOD", ["number", "divisor"], "Remainder of a division"),
    f!("MONTH", ["date"], "Month of a date"),
    f!("NOT", ["logical"], "Reverses TRUE and FALSE"),
    f!("NOW", [], "Current date and time"),
    f!("OR", ["logical1", "[logical2]…"], "TRUE if any argument is TRUE"),
    f!("POWER", ["number", "power"], "A number raised to a power"),
    f!("PRODUCT", ["number1", "[number2]…"], "Multiplies numbers"),
    f!("RIGHT", ["text", "[num_chars]"], "Last characters of text"),
    f!("ROUND", ["number", "num_digits"], "Rounds to a number of digits"),
    f!("ROUNDDOWN", ["number", "num_digits"], "Rounds toward zero"),
    f!("ROUNDUP", ["number", "num_digits"], "Rounds away from zero"),
    f!("SQRT", ["number"], "Square root"),
    f!("SUM", ["number1", "[number2]…"], "Adds numbers"),
    f!("SUMIF", ["range", "criteria", "[sum_range]"], "Adds the cells that meet a condition"),
    f!("SUMPRODUCT", ["array1", "[array2]…"], "Sum of products of ranges"),
    f!("TODAY", [], "Current date"),
    f!("TRIM", ["text"], "Removes extra spaces"),
    f!("UPPER", ["text"], "Text in upper case"),
    f!("VLOOKUP", ["lookup_value", "table", "col_index", "[range_lookup]"], "Looks up a value in the first column"),
    f!("YEAR", ["date"], "Year of a date"),
];

/// The function called `name` (any case).
pub fn function(name: &str) -> Option<&'static FunctionInfo> {
    FUNCTIONS.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

/// The function name being typed at `cursor` (a byte offset): its byte
/// range and the functions it could still become, best first (a name that
/// starts with it, alphabetical). `None` outside a formula, inside a string
/// or a reference, or when nothing is being typed.
pub fn completions(text: &str, cursor: usize) -> Option<((usize, usize), Vec<&'static FunctionInfo>)> {
    if !text.starts_with('=') || cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    if string_spans(text).iter().any(|&(a, b)| cursor > a && cursor < b) {
        return None;
    }
    let before = &text[..cursor];
    let start = before.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')).map_or(0, |i| i + 1);
    let word = &before[start..];
    // A function name starts with a letter and is not already a reference
    // or a sheet name (followed by `!`).
    if word.is_empty() || !word.starts_with(|c: char| c.is_ascii_alphabetic()) || text[cursor..].starts_with(['(', '!']) {
        return None;
    }
    let matches: Vec<_> = FUNCTIONS
        .iter()
        .filter(|f| f.name.len() > word.len() || !f.name.eq_ignore_ascii_case(word))
        .filter(|f| f.name.len() >= word.len() && f.name[..word.len()].eq_ignore_ascii_case(word))
        .collect();
    // Letters then digits (B2, AB12) is a reference being typed, unless a
    // function starts that way (LOG10).
    let looks_like_ref = parse_cell_ref(word).is_some();
    if matches.is_empty() || (looks_like_ref && matches.iter().all(|f| !f.name.eq_ignore_ascii_case(word))) && word.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(((start, cursor), matches))
}

/// `text` with the name being typed at `cursor` replaced by `f`'s name and
/// an opening parenthesis, and the cursor after it: what choosing a
/// completion does.
pub fn apply_completion(text: &str, cursor: usize, f: &FunctionInfo) -> (String, usize) {
    let Some(((start, end), _)) = completions(text, cursor) else { return (text.to_string(), cursor) };
    let mut out = String::with_capacity(text.len() + f.name.len() + 1);
    out.push_str(&text[..start]);
    out.push_str(f.name);
    out.push('(');
    let new_cursor = out.len();
    out.push_str(&text[end..]);
    (out, new_cursor)
}

/// The call the cursor is inside and which of its arguments it is in
/// (0-based): what the argument hint shows. Commas inside nested calls
/// and strings don't count.
pub fn argument_hint(text: &str, cursor: usize) -> Option<(&'static FunctionInfo, usize)> {
    if !text.starts_with('=') || cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    let strings = string_spans(text);
    let in_string = |i: usize| strings.iter().any(|&(a, b)| i > a && i < b);
    // Walk back from the cursor to the innermost unclosed '('.
    let mut depth = 0usize;
    let mut commas = 0usize;
    for (i, ch) in text[..cursor].char_indices().rev() {
        if in_string(i + 1) && ch != '"' {
            continue;
        }
        match ch {
            ')' => depth += 1,
            '(' if depth > 0 => depth -= 1,
            '(' => {
                let name_start = text[..i].rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')).map_or(0, |j| j + 1);
                return function(&text[name_start..i]).map(|f| (f, commas));
            }
            ',' | ';' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    None
}

/// The signature as `(before, current, after)`: `SUM(` `number1` `,
/// [number2]…)`, so a UI can set the current argument apart. A repeatable
/// last argument stays current however many values follow it.
pub fn signature_parts(f: &FunctionInfo, arg: usize) -> (String, String, String) {
    let n = f.args.len();
    if n == 0 {
        return (format!("{}()", f.name), String::new(), String::new());
    }
    let repeats = f.args[n - 1].ends_with('…');
    let current = if arg >= n && repeats { n - 1 } else { arg };
    if current >= n {
        return (format!("{}({})", f.name, f.args.join(", ")), String::new(), String::new());
    }
    let before = format!("{}({}", f.name, f.args[..current].iter().map(|a| format!("{a}, ")).collect::<String>());
    let after = format!("{})", f.args[current + 1..].iter().map(|a| format!(", {a}")).collect::<String>());
    (before, f.args[current].to_string(), after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_get_a_colour_each_and_a_repeat_keeps_its_colour() {
        let t = reference_tokens("=SUM(A1:B3)+C4*A1:B3-'Other'!D1+Sheet1!E2", "Sheet1", &[]);
        let spans: Vec<_> = t.iter().map(|r| (r.span, r.color)).collect();
        assert_eq!(spans, vec![((5, 10), 0), ((12, 14), 1), ((15, 20), 0), ((32, 41), 2)], "'Other'!D1 is another sheet");
        assert_eq!(t[0].rect, (0, 0, 2, 1));
        assert_eq!(distinct_ranges(&t), vec![(0, 0, 2, 1), (3, 2, 3, 2), (1, 4, 1, 4)]);
    }

    #[test]
    fn function_names_strings_and_defined_names_are_not_references() {
        assert!(reference_tokens("=LOG10(100)", "S", &[]).is_empty());
        assert!(reference_tokens("=\"see A1\"&B2", "S", &[]).iter().all(|t| t.rect == (1, 1, 1, 1)));
        assert!(reference_tokens("=Tax1*2", "S", &["TAX1".into()]).is_empty());
        assert!(reference_tokens("=ABC", "S", &[]).is_empty());
    }

    #[test]
    fn typing_a_name_offers_the_functions_it_could_be() {
        let (span, found) = completions("=SU", 3).unwrap();
        assert_eq!(span, (1, 3));
        let names: Vec<_> = found.iter().map(|f| f.name).collect();
        assert_eq!(names, ["SUM", "SUMIF", "SUMPRODUCT"]);
        let (_, found) = completions("=A1+vl", 6).unwrap();
        assert_eq!(found[0].name, "VLOOKUP", "any case, after an operator");
        assert!(completions("=SUM", 4).is_some_and(|(_, f)| f.iter().all(|f| f.name != "SUM")), "a complete name isn't offered again");
        assert!(completions("=A1", 3).is_none(), "a reference being typed");
        assert!(completions("=\"su", 4).is_none(), "inside a string");
        assert!(completions("su", 2).is_none(), "not a formula");
        assert!(completions("=SUM(", 5).is_none(), "nothing being typed");
    }

    #[test]
    fn choosing_a_completion_writes_the_name_and_an_open_paren() {
        let sum = function("sum").unwrap();
        assert_eq!(apply_completion("=1+su*2", 5, sum), ("=1+SUM(*2".to_string(), 7));
    }

    #[test]
    fn the_hint_follows_the_argument_the_cursor_is_in() {
        let text = "=IF(A1>0, SUM(B1, B2), \"a,b\")";
        let at = |needle: &str| text.find(needle).unwrap() + needle.len();
        assert_eq!(argument_hint(text, at("IF(")).map(|(f, i)| (f.name, i)), Some(("IF", 0)));
        assert_eq!(argument_hint(text, at("SUM(B1, ")).map(|(f, i)| (f.name, i)), Some(("SUM", 1)));
        assert_eq!(argument_hint(text, at("B2), ")).map(|(f, i)| (f.name, i)), Some(("IF", 2)));
        assert_eq!(argument_hint(text, at("\"a,")).map(|(f, i)| (f.name, i)), Some(("IF", 2)), "a comma in a string");
        assert_eq!(argument_hint("=A1+2", 5), None);
        assert_eq!(argument_hint("=NOSUCH(1", 9), None);
    }

    #[test]
    fn the_signature_sets_the_current_argument_apart() {
        let sum = function("SUM").unwrap();
        assert_eq!(signature_parts(sum, 0), ("SUM(".into(), "number1".into(), ", [number2]…)".into()));
        assert_eq!(signature_parts(sum, 5), ("SUM(number1, ".into(), "[number2]…".into(), ")".into()), "a repeatable argument stays current");
        let today = function("TODAY").unwrap();
        assert_eq!(signature_parts(today, 0), ("TODAY()".into(), String::new(), String::new()));
        let if_ = function("IF").unwrap();
        assert_eq!(signature_parts(if_, 4).1, "", "past the last argument: nothing current");
    }

    #[test]
    fn every_catalogued_function_is_one_the_engine_knows() {
        let mut engine = crate::engine::TablesEngine::new(4, 4).unwrap();
        for f in FUNCTIONS {
            // Enough plain arguments to satisfy the signature; the result
            // may be an argument error, but never #NAME?.
            let n = f.args.iter().filter(|a| !a.starts_with('[')).count();
            let args = vec!["1"; n].join(",");
            engine.set_cell_text(0, 0, &format!("={}({})", f.name, args));
            let got = engine.cell(0, 0);
            assert!(!got.contains("#NAME"), "{} is not a function the engine knows: {got}", f.name);
        }
        let names: Vec<_> = FUNCTIONS.iter().map(|f| f.name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "kept alphabetical");
    }
}
