use gettextrs::{gettext, ngettext};

use crate::utils::freplace;

/// Like `gettext`, but replaces named variables with the given dictionary.
///
/// The expected format to replace is `{name}`, where `name` is the first string
/// in the dictionary entry tuple.
pub fn gettext_f(msgid: &str, args: &[(&str, &str)]) -> String {
    let s = gettext(msgid);
    freplace(&s, args).into_owned()
}

/// Like `ngettext`, but replaces named variables with the given dictionary.
///
/// The expected format to replace is `{name}`, where `name` is the first string
/// in the dictionary entry tuple.
pub fn ngettext_f(msgid: &str, msgid_plural: &str, n: u32, args: &[(&str, &str)]) -> String {
    let s = ngettext(msgid, msgid_plural, n);
    freplace(&s, args).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gettext_f() {
        let out = gettext_f("{one} param", &[("one", "one")]);
        assert_eq!(out, "one param");

        let out = gettext_f("middle {one} param", &[("one", "one")]);
        assert_eq!(out, "middle one param");

        let out = gettext_f("end {one}", &[("one", "one")]);
        assert_eq!(out, "end one");

        let out = gettext_f("multiple {one} and {two}", &[("one", "1"), ("two", "two")]);
        assert_eq!(out, "multiple 1 and two");

        let out = gettext_f("multiple {two} and {one}", &[("one", "1"), ("two", "two")]);
        assert_eq!(out, "multiple two and 1");

        let out = gettext_f("multiple {one} and {one}", &[("one", "1"), ("two", "two")]);
        assert_eq!(out, "multiple 1 and 1");

        let out = ngettext_f(
            "singular {one} and {two}",
            "plural {one} and {two}",
            1,
            &[("one", "1"), ("two", "two")],
        );
        assert_eq!(out, "singular 1 and two");
        let out = ngettext_f(
            "singular {one} and {two}",
            "plural {one} and {two}",
            2,
            &[("one", "1"), ("two", "two")],
        );
        assert_eq!(out, "plural 1 and two");
    }
}
