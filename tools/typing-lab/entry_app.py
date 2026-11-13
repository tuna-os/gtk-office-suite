"""A GTK4 window with one entry, for typing-lab.py to type into.

Writes the entry's text to the file named by TYPING_LAB_OUT after every
change, and its window id to TYPING_LAB_WID once it is mapped."""
import os
import subprocess
import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk  # noqa: E402

OUT = os.environ["TYPING_LAB_OUT"]
WID = os.environ["TYPING_LAB_WID"]


def on_activate(app):
    win = Gtk.ApplicationWindow(application=app, title="typing-lab")
    entry = Gtk.Entry()
    entry.connect("changed", lambda e: open(OUT, "w").write(e.get_text()))
    win.set_child(entry)
    win.present()
    entry.grab_focus()

    def publish():
        out = subprocess.run(["xdotool", "search", "--name", "typing-lab"],
                             capture_output=True, text=True).stdout.split()
        if out:
            open(WID, "w").write(out[0])
            return False
        return True

    GLib.timeout_add(200, publish)


app = Gtk.Application(application_id="org.tunaos.TypingLab")
app.connect("activate", on_activate)
sys.exit(app.run([]))
