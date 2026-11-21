// symbol_font.rs — bullets in a symbol font, as the Unicode they show.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// PowerPoint's square and diamond bullets are Wingdings characters: `§`
// is a small square, `v` a four-diamond star. Without Wingdings installed
// (and it is not free to ship) they draw as the letters themselves. The
// model keeps the font and the character, so a save writes them back as
// they came; only the drawing takes the Unicode glyph.

/// The Unicode a bullet `ch` in `font` shows, when `font` is a symbol font
/// we know: Wingdings, Wingdings 2 or Symbol. A private-use form (`U+F0A7` for `§`, as
/// some writers store it) maps as its low byte. None for any other font,
/// or a character we have no mapping for.
pub fn unicode_bullet(font: Option<&str>, ch: &str) -> Option<&'static str> {
    let font = font?.trim().to_ascii_lowercase();
    let mut chars = ch.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let code = match c as u32 {
        c @ 0xF020..=0xF0FF => c - 0xF000,
        c @ 0x20..=0xFF => c,
        _ => return None,
    };
    match font.as_str() {
        "wingdings" => wingdings(code),
        "wingdings 2" => match code {
            // As LibreOffice draws them: a very small square, a black
            // circle and a large one (the real decks' Wingdings 2 bullets).
            0x97 => Some("\u{25CF}"),
            0x98 => Some("\u{2B24}"),
            0x9F => Some("\u{2B1D}"),
            _ => None,
        },
        "symbol" => match code {
            0xB7 => Some("\u{2022}"), // bullet
            0xA8 => Some("\u{2666}"), // black diamond suit
            0xAE => Some("\u{2192}"), // rightwards arrow
            0xD8 => Some("\u{00AC}"), // not sign
            _ => None,
        },
        _ => None,
    }
}

/// Wingdings code points that serve as bullets, after the Unicode
/// mappings LibreOffice and the Unicode Consortium's Wingdings table use.
fn wingdings(code: u32) -> Option<&'static str> {
    Some(match code {
        0x46 => "\u{261E}", // white right pointing index
        0x4A => "\u{263A}", // smiling face
        0x6C => "\u{25CF}", // black circle
        0x6D => "\u{274D}", // shadowed white circle
        0x6E => "\u{25A0}", // black square
        0x6F => "\u{25A1}", // white square
        0x70 => "\u{25A2}", // white square, rounded corners
        0x71 => "\u{2751}", // lower right shadowed white square
        0x72 => "\u{2752}", // upper right shadowed white square
        0x73 => "\u{2B27}", // black medium lozenge
        0x74 => "\u{29EB}", // black lozenge
        0x75 => "\u{25C6}", // black diamond
        0x76 => "\u{2756}", // black diamond minus white X
        0x77 => "\u{2B25}", // black medium diamond
        0x9F => "\u{2022}", // bullet
        0xA7 => "\u{25AA}", // black small square
        0xA8 => "\u{25FB}", // white medium square
        0xD8 => "\u{27A2}", // three-D top-lighted rightwards arrowhead
        0xE0 => "\u{2192}", // rightwards arrow
        0xE8 => "\u{2794}", // heavy wide-headed rightwards arrow
        0xF0 => "\u{21E8}", // rightwards white arrow
        0xFB => "\u{2718}", // heavy ballot X
        0xFC => "\u{2714}", // heavy check mark
        0xFE => "\u{2611}", // ballot box with check
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::unicode_bullet;

    #[test]
    fn wingdings_bullets_show_their_glyphs() {
        assert_eq!(unicode_bullet(Some("Wingdings"), "§"), Some("\u{25AA}"));
        assert_eq!(unicode_bullet(Some("Wingdings"), "v"), Some("\u{2756}"));
        assert_eq!(unicode_bullet(Some("Wingdings"), "ü"), Some("\u{2714}"));
        assert_eq!(unicode_bullet(Some("Wingdings"), "\u{F0A7}"), Some("\u{25AA}"), "private-use form");
        assert_eq!(unicode_bullet(Some("Symbol"), "·"), Some("\u{2022}"));
        assert_eq!(unicode_bullet(Some("Wingdings 2"), "\u{F09F}"), Some("\u{2B1D}"));
        assert_eq!(unicode_bullet(Some("Wingdings 2"), "\u{9F}"), Some("\u{2B1D}"));
    }

    #[test]
    fn other_fonts_and_unmapped_characters_are_left_alone() {
        assert_eq!(unicode_bullet(Some("Arial"), "•"), None);
        assert_eq!(unicode_bullet(Some("Courier New"), "o"), None);
        assert_eq!(unicode_bullet(None, "§"), None);
        assert_eq!(unicode_bullet(Some("Wingdings"), "A"), None);
        assert_eq!(unicode_bullet(Some("Wingdings"), "ab"), None);
    }
}
