"""GLib/GTK diagnostics in an app's stderr (#1209, release.md).

release.md asks the release to "diagnose GTK criticals/crashes". GLib logs
a programming error as `Domain-CRITICAL **: time: message` and a likely one
as `Domain-WARNING **: ...`; neither stops the app, so a journey passes
straight over them. The GUI harness reads every journey's stderr through
`parse` at teardown: a CRITICAL fails the journey, and every diagnostic is
appended to `$GUI_TEST_DIAGNOSTICS_LOG` so the run keeps
a record of the warnings too.

Measured when this went in: the full smoke suite (118 journeys) logged no
CRITICAL at all, and five WARNINGs, all from the Help dialog's unescaped
"&" in Pango markup, fixed in the same change.

Plain Python with no dogtail import, so the parser is unit-tested in the
PR lane (tests/test_gtk_diagnostics.py).
"""

import re

# `(letters:1234): Gtk-CRITICAL **: 12:00:00.000: gtk_widget_...: assertion '...' failed`
# The program prefix is optional: GLib omits it in some configurations.
LINE = re.compile(
    r"(?:\((?P<program>[^:()]+):(?P<pid>\d+)\): )?"
    r"(?P<domain>[A-Za-z][\w.-]*)-(?P<level>CRITICAL|WARNING) \*\*: "
    r"(?:[\d:.]+: )?(?P<message>.*)$"
)


def parse(stderr: str) -> list:
    """Every GLib CRITICAL and WARNING in `stderr`, in order."""
    found = []
    for line in (stderr or "").splitlines():
        m = LINE.search(line)
        if m:
            found.append({
                "domain": m.group("domain"),
                "level": m.group("level"),
                "message": m.group("message").strip(),
            })
    return found


def criticals(diagnostics: list) -> list:
    return [d for d in diagnostics if d["level"] == "CRITICAL"]
