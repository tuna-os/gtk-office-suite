// odf_formula.rs — an ods cell's formula (ODF OpenFormula) in the A1
// syntax the engine reads.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// An .ods stores `table:formula="of:=[.A1]*[$'My rates'.B2]"`: references in
// brackets, a sheet before a dot, `;` between arguments. The engine (and
// xlsx) reads `A1*'My rates'!B2` with commas. Before this the ods readers
// took only each cell's cached value, so opening an .ods and saving it
// turned every formula into a constant.
//
// A formula this cannot translate (a reference into another file, a 3D
// range across sheets) yields `None`, and the cell keeps its cached value:
// the old behaviour, for those cells only.

use calamine::Range;

/// `raw` as calamine reports it (`of:=…`, `oooc:=…`, `msoxl:=…`), in A1
/// syntax without the leading `=`.
pub(crate) fn to_a1(raw: &str) -> Option<String> {
    let body = raw.trim();
    let (ns, rest) = match body.split_once(':') {
        Some((ns, rest)) if !ns.is_empty() && ns.chars().all(|c| c.is_ascii_alphabetic()) => (ns, rest),
        _ => ("of", body),
    };
    let rest = rest.strip_prefix('=').unwrap_or(rest);
    if ns.eq_ignore_ascii_case("msoxl") {
        return Some(rest.to_string()); // already Excel syntax
    }
    let mut out = String::with_capacity(rest.len());
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                while let Some(c) = chars.next() {
                    out.push(c);
                    if c == '"' {
                        if chars.peek() == Some(&'"') {
                            out.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                }
            }
            '[' => {
                let mut inner = String::new();
                let mut quoted = false;
                loop {
                    let c = chars.next()?;
                    if c == '\'' {
                        quoted = !quoted;
                    } else if c == ']' && !quoted {
                        break;
                    }
                    inner.push(c);
                }
                out.push_str(&reference(&inner)?);
            }
            ';' => out.push(','),
            _ => out.push(c),
        }
    }
    Some(out)
}

/// The inside of `[...]`: `.A1`, `.A1:.B2`, `$Sheet1.A1`, `'My rates'.$A$1`.
fn reference(inner: &str) -> Option<String> {
    let ends = split_unquoted(inner, ':');
    if ends.is_empty() || ends.len() > 2 || inner.contains('#') {
        return None; // external file, or not a reference we know
    }
    let (sheet, first) = endpoint(ends[0])?;
    let mut out = match &sheet {
        Some(name) => format!("{}!{first}", quote(name)),
        None => first,
    };
    if let Some(second) = ends.get(1) {
        let (sheet2, cell) = endpoint(second)?;
        if sheet2.is_some() && sheet2 != sheet {
            return None; // a 3D range: no A1 equivalent
        }
        out.push(':');
        out.push_str(&cell);
    }
    Some(out)
}

/// One end of a reference: its sheet, if named, and its cell part.
fn endpoint(s: &str) -> Option<(Option<String>, String)> {
    let s = s.strip_prefix('$').unwrap_or(s);
    let (sheet, cell) = if let Some(rest) = s.strip_prefix('\'') {
        let mut name = String::new();
        let mut it = rest.char_indices();
        let end = loop {
            let (i, c) = it.next()?;
            if c == '\'' {
                if rest[i + 1..].starts_with('\'') {
                    name.push('\'');
                    it.next();
                    continue;
                }
                break i + 1;
            }
            name.push(c);
        };
        (Some(name), rest[end..].strip_prefix('.')?)
    } else if let Some(cell) = s.strip_prefix('.') {
        (None, cell)
    } else {
        let (name, cell) = s.split_once('.')?;
        (Some(name.to_string()), cell)
    };
    if cell.is_empty() || !cell.chars().all(|c| c.is_ascii_alphanumeric() || c == '$') {
        return None;
    }
    Some((sheet, cell.to_string()))
}

fn split_unquoted(s: &str, sep: char) -> Vec<&str> {
    let (mut parts, mut start, mut quoted) = (Vec::new(), 0, false);
    for (i, c) in s.char_indices() {
        if c == '\'' {
            quoted = !quoted;
        } else if c == sep && !quoted {
            parts.push(&s[start..i]);
            start = i + 1;
        }
    }
    parts.push(&s[start..]);
    parts
}

/// A sheet name as an A1 reference writes it: bare when it can be.
fn quote(name: &str) -> String {
    let bare = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if bare {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// Every formula of a sheet in A1 syntax; one that cannot be translated is
/// left empty, so the reader falls back to the cell's value.
pub(crate) fn a1_formulas(raw: &Range<String>) -> Range<String> {
    let mut out = raw.clone();
    let (r0, c0) = raw.start().unwrap_or((0, 0));
    for (row, col, f) in raw.used_cells() {
        if !f.is_empty() {
            out.set_value((r0 + row as u32, c0 + col as u32), to_a1(f).unwrap_or_default());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::to_a1;

    #[test]
    fn references_lose_their_brackets_and_dots() {
        assert_eq!(to_a1("of:=[.A1]+[.B2]").as_deref(), Some("A1+B2"));
        assert_eq!(to_a1("of:=SUM([.A1:.B3])").as_deref(), Some("SUM(A1:B3)"));
        assert_eq!(to_a1("of:=[.$A$1]*2").as_deref(), Some("$A$1*2"));
        assert_eq!(to_a1("of:=SUM([.A:.A])").as_deref(), Some("SUM(A:A)"));
    }

    #[test]
    fn a_sheet_comes_before_a_bang_quoted_when_it_must_be() {
        assert_eq!(to_a1("of:=[$Sheet1.A1]*5").as_deref(), Some("Sheet1!A1*5"));
        assert_eq!(to_a1("of:=[$'My rates'.A1]*2").as_deref(), Some("'My rates'!A1*2"));
        assert_eq!(to_a1("of:=SUM([$'Q1'.A1:.A3])").as_deref(), Some("SUM(Q1!A1:A3)"));
        assert_eq!(to_a1("of:=['Bob''s'.B2]").as_deref(), Some("'Bob''s'!B2"));
        assert_eq!(to_a1("of:=[Sheet2.A1:Sheet2.B2]").as_deref(), Some("Sheet2!A1:B2"));
    }

    #[test]
    fn separators_become_commas_outside_strings() {
        assert_eq!(to_a1("of:=IF([.A1]>1;\"a;b\";\"c\")").as_deref(), Some("IF(A1>1,\"a;b\",\"c\")"));
        assert_eq!(to_a1("of:=\"say \"\"[.A1]\"\"\"").as_deref(), Some("\"say \"\"[.A1]\"\"\""));
    }

    #[test]
    fn what_has_no_a1_form_is_none() {
        assert_eq!(to_a1("of:=['file:///x.ods'#$Sheet1.A1]"), None);
        assert_eq!(to_a1("of:=SUM([$S1.A1:$S2.A1])"), None, "a 3D range");
        assert_eq!(to_a1("of:=[.A1"), None, "unterminated");
    }

    #[test]
    fn an_excel_namespace_formula_is_already_a1() {
        assert_eq!(to_a1("msoxl:=SUM(A1:A3)").as_deref(), Some("SUM(A1:A3)"));
    }
}
