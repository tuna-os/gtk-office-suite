//! How strong a password for a Tuta recipient outside Tuta is, scored as
//! Tuta's own app scores it (`getPasswordStrength` in its
//! `PasswordUtils.ts`), so a password Tuta would warn about warns here too.
//!
//! The score counts as JavaScript does, in UTF-16 code units: a character
//! outside the Basic Multilingual Plane is two of them there, and so here.

/// The score of a full strength bar.
pub(crate) const STRENGTH_MAX: u32 = 80;

/// The lowest score Tuta calls secure. A weaker password is the user's
/// choice to confirm, never refused.
pub(crate) const STRENGTH_SECURE: u32 = 64;

/// Below this the bar shows the error color rather than the warning one
/// (Tuta's `PASSWORD_MEDIUM_INSECURE`).
pub(crate) const STRENGTH_MEDIUM: u32 = 50;

/// Keyboard rows (German and English) and the alphabet: a run of four or
/// more of these, either way round, costs strength.
const BAD_SEQUENCES: [&str; 11] = [
    "^1234567890ß´",
    "°!\"§$%&/()=?`",
    "qwertzuiopü+",
    "asdfghjklöä#",
    "<yxcvbnm,.-",
    "`1234567890-=",
    "~!@#$%^&*()_+",
    "qwertyuiop[]",
    "asdfghjkl'\\",
    "\\zxcvbnm,./",
    "abcdefghijklmnopqrstuvwxyz",
];

/// Words Tuta always counts against a password.
const BAD_STRINGS: [&str; 7] = [
    "passwort", "password", "tutanota", "free", "test", "keemail", "tutamail",
];

/// The shortest run of a sequence or bad string that counts.
const MIN_SEQUENCE_LEN: usize = 4;

fn is_lower(unit: u16) -> bool {
    matches!(unit, 0x61..=0x7a | 0x20)
}

fn is_upper(unit: u16) -> bool {
    matches!(unit, 0x41..=0x5a)
}

fn is_digit(unit: u16) -> bool {
    matches!(unit, 0x30..=0x39)
}

fn is_other(unit: u16) -> bool {
    !(is_lower(unit) || is_upper(unit) || is_digit(unit))
}

/// What JavaScript's `.` leaves out: the line terminators.
fn is_line_terminator(unit: u16) -> bool {
    matches!(unit, 0x0a | 0x0d | 0x2028 | 0x2029)
}

/// The longest run of units that all pass `class`.
fn longest_run(units: &[u16], class: impl Fn(u16) -> bool) -> usize {
    let (mut longest, mut run) = (0, 0);
    for &unit in units {
        run = if class(unit) { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

/// The longest run of one repeated unit, as `/(.)\1+/g` finds them: one
/// unit alone is no match, so it counts as none.
fn longest_repeat(units: &[u16]) -> usize {
    let mut longest = 0;
    let mut i = 0;
    while i < units.len() {
        let mut j = i + 1;
        while j < units.len() && units[j] == units[i] {
            j += 1;
        }
        if j - i >= 2 && !is_line_terminator(units[i]) {
            longest = longest.max(j - i);
        }
        i = j;
    }
    longest
}

/// How many units repeat one seen before, counting by character as
/// JavaScript's `for (const c of password)` does.
fn same_chars(password: &str, units: usize) -> usize {
    let distinct: std::collections::HashSet<char> = password.chars().collect();
    units - distinct.len()
}

fn contains(haystack: &[u16], needle: &[u16]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// How many units of `password` are in runs of four or more found in one
/// of `sequences` (reversed too, with `reverse_too`), each run taken at its
/// longest and counted once.
fn sequence_chars(password: &[u16], sequences: &[Vec<u16>], reverse_too: bool) -> usize {
    let mut all = sequences.to_vec();
    if reverse_too {
        all.extend(sequences.iter().map(|s| s.iter().rev().copied().collect()));
    }
    let mut count = 0;
    let mut i = 0;
    while i + MIN_SEQUENCE_LEN <= password.len() {
        let mut found = 0;
        let mut len = MIN_SEQUENCE_LEN;
        while i + len <= password.len() {
            let part = &password[i..i + len];
            if all.iter().any(|s| contains(s, part)) {
                found = len;
            }
            len += 1;
        }
        if found > 0 {
            count += found;
            i += found;
        } else {
            i += 1;
        }
    }
    count
}

fn utf16_lower(text: &str) -> Vec<u16> {
    text.to_lowercase().encode_utf16().collect()
}

/// The strength of `password`, 0 to [`STRENGTH_MAX`]. `bad_strings` are
/// the words that make it guessable for this recipient: the sender's own
/// addresses and name, and the recipient's address and name, as Tuta's
/// `getPasswordStrengthForUser` passes them.
pub(crate) fn strength(password: &str, bad_strings: &[&str]) -> u32 {
    let units: Vec<u16> = password.encode_utf16().collect();
    if units.is_empty() {
        return 0;
    }
    let len = units.len();
    let count = |class: fn(u16) -> bool| units.iter().filter(|&&u| class(u)).count();
    let (lower, upper, digits) = (count(is_lower), count(is_upper), count(is_digit));
    let other = len - lower - upper - digits;
    let beyond_two = |run: usize| run.saturating_sub(2) as f64;
    let consecutive = beyond_two(longest_run(&units, is_lower))
        + beyond_two(longest_run(&units, is_upper))
        + beyond_two(longest_run(&units, is_digit))
        + beyond_two(longest_run(&units, is_other))
        + beyond_two(longest_repeat(&units));
    // At best a quarter of the password is each of the four kinds.
    let quarter = len as f64 / 4.0;
    let missing = [lower, upper, digits, other]
        .iter()
        .map(|&n| (quarter - n as f64).max(0.0))
        .sum::<f64>();
    let lowered: Vec<u16> = utf16_lower(password);
    let sequences: Vec<Vec<u16>> = BAD_SEQUENCES
        .iter()
        .map(|s| s.encode_utf16().collect())
        .collect();
    let bad: Vec<Vec<u16>> = bad_strings
        .iter()
        .chain(BAD_STRINGS.iter())
        .map(|s| utf16_lower(s))
        .collect();
    let score = len as f64 * 11.0
        - missing * 3.0
        - consecutive * 2.0
        - same_chars(password, len) as f64 * 5.0
        - sequence_chars(&lowered, &sequences, true) as f64 * 4.0
        - sequence_chars(&lowered, &bad, false) as f64 * 4.0;
    // JavaScript's Math.round takes .5 up; every score here that could end
    // in one is clamped to 0 first or is positive.
    (score.round().max(0.0) as u32).min(STRENGTH_MAX)
}

/// Whether Tuta would take `strength` without asking.
pub(crate) fn is_secure(strength: u32) -> bool {
    strength >= STRENGTH_SECURE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// Scores Tuta's own `getPasswordStrength` gave for these, run under
    /// Node from its `PasswordUtils.ts`.
    #[test]
    fn scores_match_tutas_own() {
        let cases: &[(&str, &[&str], u32)] = &[
            ("", &[], 0),
            ("a", &[], 9),
            ("aX%7#+tO", &[], 80),
            ("dasisteinpasswort", &[], 57),
            ("das-ist-ein-passwort", &[], 80),
            ("testtesttesttest", &[], 0),
            ("abcdefgh", &[], 26),
            ("12345678", &[], 26),
            ("7jeGABvliT", &["7jegabvlit"], 54),
            ("7jegabvlit", &[], 77),
            ("ann@example.com", &["ann@example.com", "Ann"], 47),
            ("Grüße-Straße_42", &[], 80),
            ("İstanbul99!", &[], 80),
            ("😀😀😀😀abc", &[], 56),
            ("qwertz", &[], 21),
            ("ZXCVBNM,./", &[], 43),
            ("line\nbreak\n\nxx", &[], 80),
            ("Tr0ub", &[], 50),
            ("aX%7#", &[], 53),
            ("abc123", &[], 53),
            ("Ünïcödé1", &[], 79),
            ("zzzzzz", &[], 12),
            ("aB3$aB3$", &[], 68),
            ("pass1234", &[], 31),
            ("xY7!", &[], 44),
            ("mjudleugdh", &[], 62),
            ("!i$hL1D", &[], 75),
            ("\u{2028}\u{2028}\u{2028}x", &[], 26),
            ("ÄÖÜäöü12", &[], 68),
            ("😀a😀b", &[], 42),
            ("Sommer2024", &["sommer"], 54),
            ("qwertzuiop", &[], 32),
            ("poiuytrewq", &[], 32),
            ("9876abcd", &[], 36),
        ];
        for (password, bad, expected) in cases {
            assert_eq!(strength(password, bad), *expected, "{password:?}");
        }
    }

    #[test]
    fn bad_strings_count_whatever_their_case() {
        assert_eq!(strength("7jegabvlit", &["7jeGABvliT"]), 37);
        assert_eq!(
            strength("7JEGABVLIT", &["7jegabvlit"]),
            strength("7jegabvlit", &["7jegabvlit"])
        );
        assert_eq!(
            strength("7jegabvlit", &["7390535"]),
            strength("7jegabvlit", &[])
        );
    }

    /// Tuta's own tests of the sequence count.
    #[test]
    fn sequences_are_found_as_tutas_tests_find_them() {
        let seqs: Vec<Vec<u16>> = BAD_SEQUENCES.iter().map(|s| utf16(s)).collect();
        for (password, reverse, expected) in [
            ("qwertz", true, 6),
            ("qwerty", true, 6),
            ("4567890", true, 7),
            ("m,.-", true, 4),
            ("3kfh5678awf", true, 4),
            ("3kfhkjhgfawf", true, 5),
            ("3kfhkjhgfawf", false, 0),
            ("a2345ngb890d", true, 4),
            ("a2345890d", true, 4),
            ("r56b", true, 0),
            ("tzuio54", true, 5),
            ("54tzuio", true, 5),
        ] {
            assert_eq!(
                sequence_chars(&utf16(password), &seqs, reverse),
                expected,
                "{password}"
            );
        }
    }

    #[test]
    fn sixty_four_is_secure() {
        assert!(!is_secure(63));
        assert!(is_secure(64));
        assert!(is_secure(STRENGTH_MAX));
    }
}
