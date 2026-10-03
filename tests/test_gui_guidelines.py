"""The suite's GUI guidelines (docs/GNOME-GUIDELINES.md), held to the code.

Letters, Tables and Decks once built the same kind of dialog three ways: a
form as an alert in one app and a header-bar dialog in another, prompts
that did or didn't focus their field, buttons with and without mnemonics,
"Save document?" here and "Save changes?" there. The guidelines name one
builder for each kind of dialog, in suite_common::dialogs, and the GNOME
HIG's capitalization for the words on them. This reads the apps' sources
and fails on anything that goes around them.
"""

import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
APP_DIRS = ("letters/src", "tables/src", "decks/src", "suite-common/src")
DIALOGS_RS = "suite-common/src/dialogs.rs"

# Short words header capitalization leaves in lower case (articles,
# conjunctions and prepositions of three letters or fewer). The HIG
# capitalizes every other word: all words of four letters or more, and
# verbs and nouns of any length.
SMALL_WORDS = {"a", "an", "the", "and", "but", "or", "nor", "for", "so", "yet",
               "as", "at", "by", "in", "of", "on", "to", "up", "via", "per", "off", "out"}

# Toolkit dialogs the guidelines replace with a libadwaita one.
FORBIDDEN = {
    r"gtk4?::Dialog::": "a GtkDialog (use suite_common::dialogs)",
    r"MessageDialog": "a message dialog (use an AdwAlertDialog)",
    r"PageSetupUnixDialog": "GTK's page setup window (use suite_common::page_setup)",
    r"PrintUnixDialog": "GTK's print window (print through gtk::PrintOperation, the portal's dialog)",
    r"gtk4?::AboutDialog|AboutWindow": "an about window (use suite_common::about, an AdwAboutDialog)",
    r"PreferencesWindow": "a preferences window (use an AdwPreferencesDialog)",
    r"ShortcutsWindow::": "a GtkShortcutsWindow (use the suite's shortcuts dialog)",
    r"FileChooser(Dialog|Native)": "a file chooser window (use gtk::FileDialog, the portal's)",
}

# adw::Dialog built outside dialogs.rs: only the command palette, which is
# chromeless by design (GNOME Text Editor's and Builder's idiom).
DIALOG_BUILDERS_ALLOWED = {"suite-common/src/lib.rs": 1}

STR = r'"((?:[^"\\]|\\.)*)"'
LIT = r'&?(?:suite_common::)?(?:i18n\()?' + STR + r'\)?'


def sources():
    for d in APP_DIRS:
        for path in sorted((ROOT / d).rglob("*.rs")):
            rel = path.relative_to(ROOT).as_posix()
            text = path.read_text()
            # Unit tests build widgets of their own, and comments name
            # the dialogs they replaced.
            text = text.split("#[cfg(test)]")[0]
            text = re.sub(r"(?m)^\s*//.*$", "", text)
            yield rel, text


def header_case_problems(text):
    """The words of `text` that header capitalization would capitalize but
    `text` doesn't. Placeholders, quotations and a parenthesized shortcut
    are left alone; so is a word after "e.g." or a file extension."""
    t = re.sub(r"\\u\{[0-9a-fA-F]+\}", " ", text).replace("_", "")
    t = re.sub(r"“[^”]*”|\"[^\"]*\"|%s|\{[^}]*\}|\([^)]*\)|\.[a-z]{2,4}\b", " ", t)
    words = re.findall(r"[A-Za-z][A-Za-z'’]*(?:-[A-Za-z][A-Za-z'’]*)*", t)
    bad = []
    for i, word in enumerate(words):
        edge = i in (0, len(words) - 1)
        for part in word.split("-"):
            if part[0].islower() and (edge or part.lower() not in SMALL_WORDS):
                bad.append(word)
                break
    return bad


def strings(pattern, text):
    return [m.group(1) for m in re.finditer(pattern, text, re.S)]


def headings(text):
    """Alert headings, prompt headings and action-dialog titles."""
    out = strings(r"AlertDialog::new\(\s*Some\(\s*" + LIT, text)
    out += strings(r"\.heading\(\s*" + LIT, text)
    out += strings(r"dialogs::prompt\(\s*" + LIT, text)
    out += strings(r"dialogs::action_dialog\(\s*" + LIT, text)
    out += strings(r"dialogs::viewer_dialog\(\s*" + LIT, text)
    out += strings(r"show_error_dialog\([^;]*?,\s*" + LIT, text)
    return out


def button_labels(text):
    """Alert responses, and the action of a prompt or an action dialog."""
    out = [m.group(2) for m in re.finditer(r"add_response\(\s*" + STR + r"\s*,\s*" + LIT, text)]
    for block in strings(r"add_responses\(\s*&\[(.*?)\]\s*\)", text):
        out += [m.group(2) for m in re.finditer(r"\(\s*" + STR + r"\s*,\s*" + LIT + r"\s*\)", block)]
    out += [m.group(2) for m in re.finditer(r"dialogs::action_dialog\(\s*" + LIT + r"\s*,\s*" + LIT, text)]
    out += [m.group(1) for m in re.finditer(r"dialogs::prompt\((?:[^;]*?,){4}\s*" + LIT + r"\s*,?\s*\)", text)]
    return out


def menu_items(text):
    return strings(r"(?:\.append|MenuItem::new)\(\s*Some\(\s*" + LIT, text)


def command_labels(text):
    """The names actions are registered under: the command palette's rows."""
    out = []
    for block in strings(r"register_labels\(\s*&\[(.*?)\]\s*\)", text):
        out += [m.group(1) for m in re.finditer(r",\s*" + LIT + r"\s*\)", block)]
    return out


def tooltips(text):
    return strings(r"tooltip_text\(\s*(?:Some\(\s*)?" + LIT, text)


class HeaderCapitalizationTest(unittest.TestCase):
    """The checker itself, on the HIG's own examples."""

    def test_the_higs_examples(self):
        for good in ("Save Changes?", "Recently Used Documents", "Grid View", "Self-Test",
                     "Save as Plain Text?", "Go to Cell", "Insert Link", "Discard All",
                     "Couldn't Save “%s”", "Next Match (Enter)", "XY (Scatter)", "Save _As\\u{2026}"):
            self.assertEqual(header_case_problems(good), [], good)
        for bad, words in (("Save changes?", ["changes"]), ("Error saving file", ["saving", "file"]),
                           ("Go to cell", ["cell"]), ("Self-test", ["Self-test"]), ("is Done", ["is"])):
            self.assertEqual(header_case_problems(bad), words, bad)


class DialogKindsTest(unittest.TestCase):
    """Each dialog comes from the builder for its kind."""

    def test_no_toolkit_dialog_the_guidelines_replace(self):
        found = [f"{rel}: {why}" for rel, text in sources()
                 for pattern, why in FORBIDDEN.items() if re.search(pattern, text)]
        self.assertEqual(found, [])

    def test_dialogs_are_built_by_suite_common_dialogs(self):
        found = []
        for rel, text in sources():
            if rel == DIALOGS_RS:
                continue
            n = len(re.findall(r"adw::Dialog::(?:builder|new)\(", text))
            if n > DIALOG_BUILDERS_ALLOWED.get(rel, 0):
                found.append(f"{rel}: {n} adw::Dialog built directly (use dialogs::action_dialog or viewer_dialog)")
        self.assertEqual(found, [])

    def test_only_prompts_put_a_field_in_an_alert(self):
        found = [rel for rel, text in sources() if rel != DIALOGS_RS and "set_extra_child" in text]
        self.assertEqual(found, [], "an alert with a field of its own (use dialogs::prompt, or an action dialog for a form)")


class DialogWordsTest(unittest.TestCase):
    """The words on dialogs follow the HIG's writing style."""

    def test_headings_and_titles_use_header_capitalization(self):
        found = [f"{rel}: {h!r} {header_case_problems(h)}" for rel, text in sources()
                 for h in headings(text) if header_case_problems(h)]
        self.assertEqual(found, [])

    def test_buttons_use_header_capitalization_and_a_mnemonic(self):
        found = []
        for rel, text in sources():
            for label in button_labels(text):
                if header_case_problems(label):
                    found.append(f"{rel}: {label!r} capitalization {header_case_problems(label)}")
                if "_" not in label:
                    found.append(f"{rel}: {label!r} has no mnemonic")
        self.assertEqual(found, [])

    def test_tooltips_use_header_capitalization(self):
        found = [f"{rel}: {t!r} {header_case_problems(t)}" for rel, text in sources()
                 for t in tooltips(text) if header_case_problems(t)]
        self.assertEqual(found, [])

    def test_menu_items_use_header_capitalization(self):
        found = [f"{rel}: {m!r} {header_case_problems(m)}" for rel, text in sources()
                 for m in menu_items(text) if header_case_problems(m)]
        self.assertEqual(found, [])

    def test_command_names_use_header_capitalization(self):
        found = [f"{rel}: {c!r} {header_case_problems(c)}" for rel, text in sources()
                 for c in command_labels(text) if header_case_problems(c)]
        self.assertEqual(found, [])

    def test_the_scan_finds_the_dialogs(self):
        """Guard the scan itself: if the patterns stop matching, every test
        above passes on nothing."""
        all_headings = [h for _, text in sources() for h in headings(text)]
        all_buttons = [b for _, text in sources() for b in button_labels(text)]
        for expected in ("Save Changes?", "Rename Sheet", "Insert Link", "Page Setup", "Headers and Footers"):
            self.assertIn(expected, all_headings)
        for expected in ("_Cancel", "_Rename", "_Apply", "_Discard"):
            self.assertIn(expected, all_buttons)
        all_menus = [m for _, text in sources() for m in menu_items(text)]
        self.assertIn("_Preferences", all_menus)
        all_commands = [c for _, text in sources() for c in command_labels(text)]
        self.assertGreater(len(all_commands), 100)


if __name__ == "__main__":
    unittest.main()
