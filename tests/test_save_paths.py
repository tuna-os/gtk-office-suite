"""Every path that reads or writes a document goes through the
compatibility boundary, and docs/SAVE-PATHS.md lists each one (#1273).

The apps' document I/O funnels through a few entry points per app (SINKS
below). This scans the app sources (test modules excluded) for every call
to them and checks it against the table in docs/SAVE-PATHS.md:

- every call site has a row, and every row a call site: a new path can't
  be added without saying how it treats the format's losses;
- a row whose boundary is `asks` really is behind the loss question: the
  call is inside the save closure handed to the app's guard
  (`save_after_asking`, Letters' `save_asking_about_loss`), or inside the
  guard itself;
- every test a row cites exists.

Run it directly with --list to print the call sites it finds.
"""

import os
import re
import sys
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOC = os.path.join(ROOT, "docs", "SAVE-PATHS.md")

# Per app: the guard functions that ask about losses before a save, and the
# document I/O entry points whose every call site must be in the table.
GUARDS = {
    "letters": ["save_asking_about_loss"],
    "tables": ["save_after_asking"],
    "decks": ["save_after_asking"],
}
SINKS = {
    "letters": [
        "save_page_to_path",      # Save, Save As, close guard (window/saving.rs)
        "save_buffer_to_file",    # the one writer of a user's file
        "load_file_to_buffer",    # every open
        "autosave_slot.write",    # recovery snapshots
        "engine::export_pdf",     # Export as PDF with Typst
        "write_pdf",              # Export as PDF from Print Layout, and the
                                  # test-mode render dump / headless export
    ],
    "tables": [
        "save_engine_to_xlsx",
        "save_sheets_to_xlsx_with_engine",
        "load_workbook",
        "slot.write",
        "to_pdf_with_setup",
    ],
    "decks": [
        "save_deck",
        "write_deck",
        "read_deck",
        "slot.write",
        "export_pdf",
        "export_png",
    ],
}
BOUNDARIES = {"asks", "snapshot", "export", "read", "helper"}


def app_sources(app):
    base = os.path.join(ROOT, app, "src")
    for dirpath, _, files in os.walk(base):
        for f in sorted(files):
            if f.endswith(".rs") and f != "tests.rs":
                yield os.path.relpath(os.path.join(dirpath, f), ROOT)


def blank_comments_and_literals(text):
    """Comments, string and char literals blanked to spaces (newlines
    kept), so offsets and line numbers hold and a "(" in a message can't
    unbalance the bracket count. A small scanner rather than regexes: a
    quote inside a comment, or an apostrophe in prose, would throw a regex
    off."""
    out, i, n = list(text), 0, len(text)

    def blank(a, b):
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = text[i]
        if text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j == -1 else j
            blank(i, j)
            i = j
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            j = n if j == -1 else j + 2
            blank(i, j)
            i = j
        elif c == "r" and re.match(r'r#*"', text[i:]) and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            hashes = re.match(r"r(#*)\"", text[i:]).group(1)
            j = text.find('"' + hashes, i + 2 + len(hashes))
            j = n if j == -1 else j + 1 + len(hashes)
            blank(i, j)
            i = j
        elif c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            blank(i, j + 1)
            i = j + 1
        elif c == "'":
            # A char literal ('x', '\n', '\u{..}'), not a lifetime ('a).
            m = re.match(r"'(\\u\{[0-9a-fA-F]+\}|\\.|[^\\'])'", text[i:])
            if m:
                blank(i, i + m.end())
                i += m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def strip_tests_and_comments(text):
    """The file without its test module, comments or literals, same length
    (so offsets and line numbers still hold)."""
    text = blank_comments_and_literals(text)
    m = re.search(r"#\[cfg\(test\)\]\s*mod \w+\s*\{", text)
    if m:
        text = text[: m.start()] + re.sub(r"[^\n]", " ", text[m.start():])
    return text


def call_sites(app):
    """(file, sink, ordinal, offset, text) for every call of a sink. A
    definition (`fn sink(`) is not a call."""
    out = []
    for path in app_sources(app):
        text = strip_tests_and_comments(open(os.path.join(ROOT, path), encoding="utf-8").read())
        for sink in SINKS[app]:
            n = 0
            for m in re.finditer(re.escape(sink) + r"[a-z_]*\(", text):
                if re.search(r"fn\s+$", text[max(0, m.start() - 8): m.start()]):
                    continue
                # `save_deck(` must not match inside `autosave_deck(`.
                if m.start() > 0 and (text[m.start() - 1].isalnum() or text[m.start() - 1] == "_") and "." not in sink and "::" not in sink:
                    continue
                n += 1
                out.append((path, sink, n, m.start(), text))
    return out


def inside_guard(text, offset, guards):
    """Whether `offset` is inside the argument list of a guard call, or
    inside the body of a guard function."""
    for g in guards:
        for m in re.finditer(r"\b" + g + r"\s*\(", text[:offset]):
            if re.search(r"fn\s+$", text[max(0, m.start() - 8): m.start()]):
                # The guard's own body: from its `{` to the matching `}`.
                body = text.find("{", m.end())
                if body != -1 and body < offset and depth(text, body, offset, "{", "}") > 0:
                    return True
                continue
            if depth(text, m.end() - 1, offset, "(", ")") > 0:
                return True
    return False


def depth(text, start, end, open_c, close_c):
    """How deep `end` is inside the bracket opened at `start`: 0 once that
    bracket has closed, however many open after it."""
    d = 0
    for c in text[start:end]:
        if c == open_c:
            d += 1
        elif c == close_c:
            d -= 1
            if d == 0:
                return 0
    return d


def table_rows():
    """Rows of the call-site table: (app, file, sink, ordinal, boundary, test)."""
    rows = []
    for line in open(DOC, encoding="utf-8"):
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) != 7 or cells[0] not in SINKS:
            continue
        app, _path_name, file, sink, ordinal, boundary, test = cells
        rows.append((app, file.strip("`"), sink.strip("`"), int(ordinal), boundary.strip("`"), test.strip("`")))
    return rows


def cited_test_exists(name):
    """A cited test: `file::function` or `file::Class`. A path no test
    covers says `none:` and names the issue or PR that tracks it."""
    if name.startswith("none:"):
        return re.search(r"#\d+", name) is not None
    if "::" in name:
        file, fn = name.rsplit("::", 1)
        path = os.path.join(ROOT, file)
        return os.path.exists(path) and re.search(r"\b(fn|class|def)\s+" + re.escape(fn) + r"\b", open(path, encoding="utf-8").read())
    return False


class SavePaths(unittest.TestCase):
    def test_every_call_site_has_a_row_and_every_row_a_call_site(self):
        found = {(app, f, s, n) for app in SINKS for (f, s, n, _, _) in call_sites(app)}
        listed = {(app, f, s, n) for (app, f, s, n, _, _) in table_rows()}
        self.assertEqual(sorted(found - listed), [], "call sites docs/SAVE-PATHS.md doesn't list")
        self.assertEqual(sorted(listed - found), [], "rows with no call site (moved or renamed?)")

    def test_asks_rows_are_behind_the_loss_question(self):
        sites = {(app, f, s, n): (off, text) for app in SINKS for (f, s, n, off, text) in call_sites(app)}
        wrong = []
        for app, f, s, n, boundary, _ in table_rows():
            self.assertIn(boundary, BOUNDARIES, f"{f} {s} #{n}")
            if boundary == "asks" and (app, f, s, n) in sites:
                off, text = sites[(app, f, s, n)]
                if not inside_guard(text, off, GUARDS[app]):
                    line = text[:off].count("\n") + 1
                    wrong.append(f"{f}:{line} {s} (#{n})")
        self.assertEqual(wrong, [], "rows marked `asks` whose call isn't behind the loss question")

    def test_every_cited_test_exists(self):
        missing = [t for *_, t in table_rows() if not cited_test_exists(t)]
        self.assertEqual(missing, [])


if __name__ == "__main__":
    if "--list" in sys.argv:
        for app in SINKS:
            for f, s, n, off, text in call_sites(app):
                line = text[:off].count("\n") + 1
                guarded = inside_guard(text, off, GUARDS[app])
                print(f"{app} {f}:{line} {s} #{n} {'guarded' if guarded else ''}")
    else:
        unittest.main()
