#!/usr/bin/env python3
"""Live collaboration in Letters (RFC-0005 P5): two real instances of the app,
each with its own sidecar and identity, edit one document through a Durable
Streams server.

What the journeys prove is read from AT-SPI, never from the app's own
bookkeeping: menu items and dialog rows by their accessible names, the
document by the PageView's text, its formatting by the PageView's text
attributes, the sharing state by the status button's name, and "nothing ran"
by the app's process tree. Every wait names the state it waits for
(`wait_until`), so a timeout reports the last thing it saw.

`LettersCollabStreams` needs a Durable Streams server (DCP_STREAMS_URL, from
`eval "$(tools/collab-spike/scripts/streams-up.sh node)"`) and fails without
one; DCP_SKIP_STREAMS=1 skips it, saying so. It also needs the sidecar
(`cd collab && cargo build -p collabd --locked`; COLLABD_BIN) and Letters built
with `--features collab`.

Both windows are shown side by side in one recording when the run uses a window
manager that places windows: GUI_TEST_WINDOW_MANAGER=openbox
(tests/gui/run_gui_tests.sh). Under matchbox, the default, each window takes the
whole screen and the recording shows whichever instance the journey last
activated; the assertions are the same.
"""

import os
import re
import time

from dogtail import rawinput

from framework import BaseGUITestCase, instances

ELLIPSIS = "\u2026"
EDITABLE_ROLES = ("text", "entry", "password text")
BANNER = "Two versions of this document exist"
RESOLVE_HEADING = "Two Versions of This Document"
STATUS_ROLES = ("toggle button", "push button", "menu button")

# Waits that involve the server (a stream created, a snapshot fetched, an
# update delivered) are bounded by a slow runner, not by the app.
NETWORK_SECONDS = 90.0


def _safe(call, default=None):
    """`call()`, or `default` when the node went away mid-walk: an AT-SPI
    tree changes while it is read, and a stale node is not a verdict."""
    try:
        return call()
    except Exception:
        return default


class _LettersAtspi:
    """What the Letters journeys read and press, by accessible name. Mixed
    into a BaseGUITestCase; every helper takes the instance (a
    `focus_app` handle) it is about, so two instances never answer for each
    other."""

    def _first(self, handle, predicate):
        return handle.app.findChild(
            lambda node: bool(_safe(lambda: predicate(node), False)),
            retry=False, requireResult=False)

    def _all(self, handle, predicate):
        return handle.app.findChildren(
            lambda node: bool(_safe(lambda: predicate(node), False)))

    @staticmethod
    def _text_of(node):
        text = node.queryText()
        return text.getText(0, text.characterCount)

    # ── menus and buttons ──────────────────────────────────────────────

    def _main_menu_toggle(self, handle):
        return self.wait_until(
            lambda: self._first(handle, lambda n: n.name == "Main Menu"
                                and n.roleName == "toggle button" and n.showing),
            lambda node: node is not None, interval=0.25,
            description=f"the Main Menu button of {handle}")

    # GTK 4.14 gives a popover menu's items no accessible names (the cells'
    # context-menu journey in test_smoke.py says the same), so the items are
    # found by position: the main menu ends with Preferences, Help, Keyboard
    # Shortcuts and About, and the three Collaborate items sit just before them.
    MENU_OFFSET = {"Share This Document" + ELLIPSIS: -7,
                   "Join a Shared Document" + ELLIPSIS: -6,
                   "Stop Sharing": -5}

    def _menu_item(self, handle, label, timeout=10.0, need_action=True):
        """The Collaborate menu item labelled `label`, by its place in the open menu."""
        def find():
            items = [n for n in self._all(handle, lambda n: n.roleName == "menu item" and n.showing)]
            items.sort(key=lambda n: _safe(lambda n=n: n.position[1], 0))
            if len(items) < 8:
                return None
            return items[self.MENU_OFFSET[label]]
        return self.wait_until(find, lambda node: node is not None, timeout=timeout,
                               interval=0.25,
                               description=f"the menu item {label!r} of {handle} (by position)")

    def _open_main_menu(self, handle):
        self.focus_app(handle)
        self._main_menu_toggle(handle).do_action(0)

    def _choose_from_main_menu(self, handle, label):
        """Open the Main Menu and press `label`, which closes the menu."""
        self._open_main_menu(handle)
        self._menu_item(handle, label).do_action(0)

    @staticmethod
    def _is_sensitive(node):
        import pyatspi
        return node.getState().contains(pyatspi.STATE_SENSITIVE)

    def _close_main_menu(self, handle):
        self.focus_app(handle)
        rawinput.keyCombo("Escape")
        self.wait_until(
            lambda: self._first(handle, lambda n: n.roleName == "menu item" and n.showing),
            lambda node: node is None, interval=0.25,
            description=f"the Main Menu of {handle} to close")

    # ── dialogs ────────────────────────────────────────────────────────

    # The rows in the order the dialogs show them (collab_dialogs.rs), for when
    # the toolkit gives an entry row's field no accessible name.
    ROW_INDEX = {"Server": 0, "People (public keys)": 1, "Your Key": 1, "Join Code": 3}

    def _row(self, handle, title):
        """The text field of the entry row titled `title`: the node with that
        accessible name when it is the field (or holds it), else the field at the
        row's place among the showing fields of the dialog (not the page view)."""
        def editable(node):
            return node.roleName in EDITABLE_ROLES and node.showing and node.name != "Print Layout"

        def find():
            for node in self._all(handle, lambda n: n.name == title and n.showing):
                if _safe(lambda n=node: editable(n), False):
                    return node
                inner = _safe(lambda n=node: n.findChild(
                    lambda c: bool(_safe(lambda: editable(c), False)),
                    retry=False, requireResult=False))
                if inner is not None:
                    return inner
            fields = [n for n in self._all(handle, editable)]
            fields.sort(key=lambda n: _safe(lambda n=n: n.position[1], 0))
            index = self.ROW_INDEX[title]
            # The Share dialog has three fields, the Join dialog four.
            if len(fields) >= (4 if title in ("Your Key", "Join Code") else 3) and index < len(fields):
                return fields[index]
            return None
        return self.wait_until(find, lambda node: node is not None, interval=0.25,
                               description=f"the row {title!r} of {handle}")

    def _row_text(self, handle, title):
        return self._text_of(self._row(handle, title))

    def _fill_row(self, handle, title, value):
        """Put `value` in the row and wait until the row says so: set through
        the field's AT-SPI text interface, and typed as a user would if the
        interface did not take it."""
        import pyatspi
        field = self._row(handle, title)
        self.focus_app(handle)
        _safe(lambda: field.queryComponent().grabFocus())
        # Best effort: the value is set through the field's text interface, and
        # typed only when that did not take (below), where focus matters.
        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline and not field.getState().contains(pyatspi.STATE_FOCUSED):
            time.sleep(0.1)  # settling: waiting for a state change the toolkit may never report
        _safe(lambda: field.queryEditableText().setTextContents(value))
        if _safe(lambda: self._text_of(field)) != value:
            rawinput.keyCombo("<Control>a")
            rawinput.typeText(value)
        self.wait_until(lambda: self._text_of(field), lambda text: text == value,
                        description=f"the row {title!r} of {handle} to hold {value!r}")
        return field

    def _dialog_button(self, handle, heading, label):
        """The push button `label` of the dialog headed `heading`. The dialog
        is found by its heading and the button inside it, so another
        `Close` or `Share` elsewhere in the window is not pressed."""
        def is_button(node):
            # A response button's mnemonic underscore may be in its name (`_Keep Mine`).
            return (node.name or "").replace("_", "") == label and node.roleName == "push button" and node.showing

        def find():
            anchor = self._first(handle, lambda n: n.name == heading and n.showing)
            node = anchor
            for _ in range(5):
                if node is None:
                    break
                hit = _safe(lambda n=node: n.findChild(
                    lambda c: bool(_safe(lambda: is_button(c), False)),
                    retry=False, requireResult=False))
                if hit is not None:
                    return hit
                node = _safe(lambda n=node: n.parent)
            if anchor is None:
                # The heading is not exposed under that name: fall back to the
                # button alone, but only when it is unambiguous.
                buttons = self._all(handle, is_button)
                if len(buttons) == 1:
                    return buttons[0]
            return None
        return self.wait_until(find, lambda node: node is not None, interval=0.25,
                               description=f"the {label!r} button of the dialog "
                                           f"{heading!r} of {handle}")

    def _label_with(self, handle, text):
        """The text of the first showing label whose text contains `text`."""
        node = self._first(handle, lambda n: n.roleName == "label" and n.showing
                           and text in (n.name or ""))
        return node.name if node is not None else None

    # ── the document ───────────────────────────────────────────────────

    def _page(self, handle):
        return self._first(handle, lambda n: n.name == "Print Layout"
                           and n.roleName == "text" and n.showing)

    def _wait_page(self, handle):
        return self.wait_until(lambda: self._page(handle), lambda node: node is not None,
                               timeout=30.0, interval=0.25,
                               description=f"the page view of {handle}")

    def _page_text(self, handle):
        page = self._page(handle)
        if page is None:
            raise AssertionError(f"{handle} shows no page view")
        return self._text_of(page)

    def _focus_page(self, handle):
        """Send the keyboard to the instance's page view and wait for it."""
        import pyatspi
        self.focus_app(handle)
        page = self._wait_page(handle)
        _safe(lambda: page.queryComponent().grabFocus())
        # After a dialog the keyboard is on whatever the dialog was opened from,
        # and AT-SPI reports no extents for the page view, so click the middle of
        # the window, where the page is, as a person would.
        import subprocess
        geometry = dict(line.split("=", 1) for line in subprocess.run(
            ["xdotool", "getwindowgeometry", "--shell", str(handle.win_id)],
            capture_output=True, text=True, timeout=5).stdout.split())
        x = int(geometry["X"]) + int(geometry["WIDTH"]) // 2
        y = int(geometry["Y"]) + int(geometry["HEIGHT"]) // 2
        subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], capture_output=True, timeout=5)
        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline and not page.getState().contains(pyatspi.STATE_FOCUSED):
            time.sleep(0.1)  # settling: the toolkit may never report focus for the page view

    def _runs(self, handle):
        """(start, end, attributes) for each formatting run of the page text,
        as the page view reports them over AT-SPI (`Text.get_attributes`)."""
        text = self._wait_page(handle).queryText()

        def attributes(offset):
            getter = getattr(text, "get_attributes", None) or text.getAttributes
            return getter(offset)

        return instances.text_runs(attributes, text.characterCount)

    @staticmethod
    def _is_bold(attrs):
        return attrs.get("weight") == "700"

    def _bold_states(self, handle):
        return [(start, end, self._is_bold(attrs))
                for start, end, attrs in self._runs(handle)]

    def _require_streams(self):
        """The server's URL, or the skip / failure the harness promises. Before
        anything is launched, so a run with no server does not start two
        applications to say so."""
        self.streams_url = os.environ.get("DCP_STREAMS_URL")
        if not self.streams_url:
            if os.environ.get("DCP_SKIP_STREAMS") == "1":
                self.skipTest(
                    "SKIPPED (DCP_SKIP_STREAMS=1): DCP_STREAMS_URL is not set, "
                    "so there is no Durable Streams server for the two instances")
            self.fail(
                "DCP_STREAMS_URL is not set: start the reference server "
                "(eval \"$(tools/collab-spike/scripts/streams-up.sh node)\") or set "
                "DCP_SKIP_STREAMS=1 to skip this journey explicitly")

    # ── the banner and the Resolve dialog ──────────────────────────────

    def _banner(self, handle):
        """The revealed banner. An unrevealed AdwBanner stays in the tree, named
        and `showing`, with no height ((0, 0) (1910, 0) in a tree dump), so a
        banner is there only when it has height."""
        return self._first(handle, lambda n: n.showing and BANNER in (n.name or "")
                           and n.size[1] > 0)

    # ── sharing state ──────────────────────────────────────────────────

    def _status(self, handle):
        """The name of the sharing status button, `Sharing: <state>, <n>
        people`, or None while the button is hidden."""
        node = self._first(handle, lambda n: n.roleName in STATUS_ROLES and n.showing
                           and (n.name or "").startswith("Sharing:"))
        return node.name if node is not None else None

    def _wait_online(self, handle):
        return self.wait_until(
            lambda: self._status(handle),
            lambda name: bool(name) and name.startswith("Sharing: online"),
            timeout=NETWORK_SECONDS, interval=0.25,
            description=f"the status of {handle} to read 'Sharing: online'")


class LettersCollabStreams(_LettersAtspi, BaseGUITestCase):
    app_name = "letters"

    def setUp(self):
        self._require_streams()
        self.prepare_instance_pair()
        super().setUp()

    def test_two_instances_converge(self):
        import pyatspi  # noqa: F401  (a missing binding fails here, not midway)

        a, b = self.launch_instance_pair()
        if os.environ.get("GUI_TEST_WINDOW_MANAGER", "matchbox") != "matchbox":
            self.assertTrue(
                self.windows_tiled,
                "the window manager was asked to place windows and the two "
                "Letters windows are not side by side")

        # A: a new document holding "Hello".
        self.focus_app(a)
        rawinput.keyCombo("<Control>n")
        self._focus_page(a)
        rawinput.typeText("Hello")
        self.wait_until(lambda: self._page_text(a), lambda text: text.rstrip("\n") == "Hello",
                        description="A's page to hold 'Hello'")

        # B: the Join dialog; the Server row, then Enter, makes B's key.
        self._choose_from_main_menu(b, "Join a Shared Document" + ELLIPSIS)
        self._fill_row(b, "Server", self.streams_url)
        rawinput.keyCombo("Return")
        b_key = self.wait_until(
            lambda: self._row_text(b, "Your Key"),
            lambda text: text.startswith("ed25519:"),
            timeout=NETWORK_SECONDS, interval=0.25,
            description="B's 'Your Key' row to show a public key (ed25519:...)")

        # A: share with B's key, read the join code off the alert.
        self._choose_from_main_menu(a, "Share This Document" + ELLIPSIS)
        self._fill_row(a, "Server", self.streams_url)
        self._fill_row(a, "People (public keys)", b_key)
        self._dialog_button(a, "Share This Document", "Share").do_action(0)
        body = self.wait_until(
            lambda: self._label_with(a, "streams:"),
            lambda text: bool(text), timeout=NETWORK_SECONDS, interval=0.25,
            description="A's 'Sharing Started' alert to show the join code (streams:...)")
        code = re.search(r"streams:\S+", body).group(0)
        self._dialog_button(a, "Sharing Started", "Close").do_action(0)
        self.wait_until(lambda: self._label_with(a, "streams:"), lambda text: text is None,
                        description="A's 'Sharing Started' alert to close")

        # B: the code in the Join dialog, then Join; the shared document opens
        # in a new tab.
        self._fill_row(b, "Join Code", code)
        self._dialog_button(b, "Join a Shared Document", "Join").do_action(0)
        self.wait_until(
            lambda: self._label_with(b, "Joined "),
            lambda text: bool(text) and text.startswith("Joined "),
            timeout=NETWORK_SECONDS, interval=0.1,
            description="B's 'Joined “<title>”' toast")
        self.wait_until(lambda: self._page_text(b),
                        lambda text: text.rstrip("\n") == "Hello",
                        timeout=NETWORK_SECONDS, interval=0.25,
                        description="B's new tab to show 'Hello'")

        # Both are online, and each says so.
        self._wait_online(a)
        self._wait_online(b)

        # Both type into the same paragraph, each at its own end, before
        # either has seen the other's edit.
        self._focus_page(a)
        rawinput.keyCombo("End")
        rawinput.typeText(" from A")
        # What each typed lands in its own page first: wait for that before the
        # keyboard moves to the other window, so a key that never arrived is
        # reported as that (with the page's text), not as a failure to converge.
        self.wait_until(lambda: self._page_text(a), lambda t: t.rstrip("\n").endswith(" from A"),
                        description="A's own page to show what A typed")
        self._focus_page(b)
        rawinput.keyCombo("Home")
        rawinput.typeText("B says ")
        self.wait_until(lambda: self._page_text(b), lambda t: "B says " in t,
                        description="B's own page to show what B typed")

        def both():
            return self._page_text(a), self._page_text(b)

        text_a, text_b = self.wait_until(
            both,
            lambda texts: texts[0] == texts[1]
            and " from A" in texts[0] and "B says " in texts[0] and "Hello" in texts[0],
            timeout=NETWORK_SECONDS, interval=0.25,
            description="A and B to show one text holding 'Hello', ' from A' and 'B says '")
        self.assertEqual(text_a, text_b)
        self.assertEqual(text_a.count("Hello"), 1, text_a)
        self.assertEqual(text_a.count(" from A"), 1, text_a)
        self.assertEqual(text_a.count("B says "), 1, text_a)
        # Nothing else: no lost character, no duplicated one.
        self.assertEqual(len(text_a.rstrip("\n")),
                         len("Hello") + len(" from A") + len("B says "), text_a)

        # B has formatted nothing: no character of its page is bold yet.
        before = self._bold_states(b)
        self.assertTrue(before, "B's page reports no attribute runs")
        self.assertFalse([run for run in before if run[2]],
                         f"B's text is bold before anyone made it so: {before}")

        # A selects everything and makes it bold; B's AT-SPI attributes follow.
        self._focus_page(a)
        rawinput.keyCombo("<Control>a")
        rawinput.keyCombo("<Control>b")
        runs = self.wait_until(
            lambda: self._bold_states(b),
            lambda states: bool(states) and all(bold for _s, _e, bold in states),
            timeout=NETWORK_SECONDS, interval=0.25,
            description="every character of B's page to report weight 700")
        self.assertEqual(runs[0][0], 0, runs)
        self.assertEqual(runs[-1][1], len(text_a), runs)
        self.assertEqual(self._page_text(b), text_a, "bold changed B's text")
        self.assertTrue(all(bold for _s, _e, bold in self._bold_states(a)),
                        "A's own text is not all bold")

        # A stops sharing: its status button goes.
        self._choose_from_main_menu(a, "Stop Sharing")
        self.wait_until(lambda: self._status(a), lambda name: name is None,
                        timeout=30.0, interval=0.25,
                        description="A's status button to be hidden after Stop Sharing")
        self.assert_still_running(b)


class LettersCollabTwoHeads(_LettersAtspi, BaseGUITestCase):
    """Two versions nobody here has merged: the banner, and the three ways out.

    A opens a .docx, shares it with B and with a third person (`CollabPeer`, a
    real sidecar with no window); B joins. A edits and saves, which publishes a
    version A holds. The third person then saves a different version made from
    what they hold, which is nothing: a head that covers none of the live
    document. Both windows say so in a banner. Each journey takes one way out
    through the Resolve dialog and reads the outcome from AT-SPI on both
    windows (the banner, the page's text), and the version graph from the
    third person's own sidecar (its last `heads` event)."""

    app_name = "letters"
    BASE = "Base"
    MINE = "Base mine"
    THEIRS = "Theirs text"

    def setUp(self):
        self._require_streams()
        from collab_peer import docx_bytes
        self.path = os.path.join(self.temp_dir("collab-two-heads-"), "letter.docx")
        with open(self.path, "wb") as f:
            f.write(docx_bytes(self.BASE))
        self.launch_args = [self.path]
        self.prepare_instance_pair()
        super().setUp()

    def _text_is(self, handle, expected, description):
        return self.wait_until(lambda: self._page_text(handle),
                               lambda text: text.rstrip("\n") == expected,
                               timeout=NETWORK_SECONDS, interval=0.25,
                               description=f"{description}: the page of {handle} to hold {expected!r}")

    def _two_heads(self):
        """Share, join, save, and make the second head. Returns (a, b, peer)."""
        from collab_peer import CollabPeer, docx_bytes

        a, b = self.launch_instance_pair()
        peer = CollabPeer(self._collabd_bin, self.temp_dir("collab-peer-"))
        self.addCleanup(peer.close)
        peer_key = peer.connect(self.streams_url)
        self._text_is(a, self.BASE, "A opened the file")

        # B: the Join dialog, for B's key.
        self._choose_from_main_menu(b, "Join a Shared Document" + ELLIPSIS)
        self._fill_row(b, "Server", self.streams_url)
        rawinput.keyCombo("Return")
        b_key = self.wait_until(
            lambda: self._row_text(b, "Your Key"), lambda text: text.startswith("ed25519:"),
            timeout=NETWORK_SECONDS, interval=0.25,
            description="B's 'Your Key' row to show a public key (ed25519:...)")

        # A: share with B and with the third person.
        self._choose_from_main_menu(a, "Share This Document" + ELLIPSIS)
        self._fill_row(a, "Server", self.streams_url)
        self._fill_row(a, "People (public keys)", f"{b_key}, {peer_key}")
        self._dialog_button(a, "Share This Document", "Share").do_action(0)
        body = self.wait_until(
            lambda: self._label_with(a, "streams:"), lambda text: bool(text),
            timeout=NETWORK_SECONDS, interval=0.25,
            description="A's 'Sharing Started' alert to show the join code (streams:...)")
        code = re.search(r"streams:\S+", body).group(0)
        self._dialog_button(a, "Sharing Started", "Close").do_action(0)
        self.wait_until(lambda: self._label_with(a, "streams:"), lambda text: text is None,
                        description="A's 'Sharing Started' alert to close")

        self._fill_row(b, "Join Code", code)
        self._dialog_button(b, "Join a Shared Document", "Join").do_action(0)
        self._text_is(b, self.BASE, "B joined")
        peer.join(code)
        self._wait_online(a)
        self._wait_online(b)

        # A edits and saves: a version A holds, so no conflict yet.
        self._focus_page(a)
        rawinput.keyCombo("<Control>End")
        rawinput.typeText(" mine")
        self._text_is(a, self.MINE, "A typed")
        self._text_is(b, self.MINE, "A's edit reached B")
        rawinput.keyCombo("<Control>s")
        self.wait_until(lambda: self._label_with(a, "Saved and shared"), bool,
                        timeout=NETWORK_SECONDS, interval=0.1,
                        description="A's 'Saved and shared' toast")
        self.assertIsNone(self._banner(a), "the banner showed with a single version")
        self.assertIsNone(self._banner(b), "the banner showed with a single version")

        # The third person saves from what they hold, which is nothing.
        peer.publish_version(docx_bytes(self.THEIRS), message="saved from an older copy")
        for handle in (a, b):
            self.wait_until(lambda h=handle: self._banner(h), lambda node: node is not None,
                            timeout=NETWORK_SECONDS, interval=0.25,
                            description=f"the two-versions banner on {handle}")
        heads, conflict = self.wait_until(
            peer.latest_heads, lambda state: state is not None and len(state[0]) == 2,
            timeout=NETWORK_SECONDS, interval=0.25,
            description="the third person's sidecar to report two heads")
        self.assertTrue(conflict, "two heads, one of them uncovered, and no conflict reported")
        return a, b, peer

    def _resolve(self, handle, choice):
        """Banner button, then `choice` in the dialog it opens."""
        button = self._first(handle, lambda n: n.roleName == "push button" and n.showing
                             and (n.name or "") == "Resolve" + ELLIPSIS)
        self.assertIsNotNone(button, f"the banner of {handle} has no Resolve{ELLIPSIS} button")
        button.do_action(0)
        body = self.wait_until(
            lambda: self._label_with(handle, "\u2022"), bool, interval=0.25,
            description="the Resolve dialog to list the versions")
        self.assertEqual(body.count("\u2022"), 2, f"the Resolve dialog lists {body!r}")
        self._dialog_button(handle, RESOLVE_HEADING, choice).do_action(0)
        self.wait_until(lambda: self._label_with(handle, "\u2022"), lambda text: text is None,
                        description="the Resolve dialog to close")

    def _one_head(self, a, b, peer, text):
        """Both windows lost the banner, show `text`, and the graph has one head."""
        for handle in (a, b):
            self.wait_until(lambda h=handle: self._banner(h), lambda node: node is None,
                            timeout=NETWORK_SECONDS, interval=0.25,
                            description=f"the banner of {handle} to go")
        self._text_is(a, text, "resolved")
        self._text_is(b, text, "resolved")
        heads, conflict = self.wait_until(
            peer.latest_heads, lambda state: state is not None and len(state[0]) == 1,
            timeout=NETWORK_SECONDS, interval=0.25,
            description="the third person's sidecar to report one head")
        self.assertFalse(conflict)
        self.assert_still_running(a, b)

    def test_keep_mine_collapses_the_heads(self):
        a, b, peer = self._two_heads()
        self._resolve(a, "Keep Mine")
        self._one_head(a, b, peer, self.MINE)

    def test_keep_theirs_takes_the_other_version_in(self):
        a, b, peer = self._two_heads()
        self._resolve(a, "Keep Theirs")
        self._one_head(a, b, peer, self.THEIRS)

    def test_open_both_leaves_the_shared_session_alone(self):
        a, b, peer = self._two_heads()
        before, _ = peer.latest_heads()
        self._resolve(a, "Open Both")
        # The other version is a document of its own, in a tab of A.
        self._text_is(a, self.THEIRS, "the other version opened")
        # Nothing observable says when "nothing was published" is certain.
        # settling: the check below is that the graph did not change after the tab opened
        time.sleep(3.0)
        self.assertEqual(sorted(peer.latest_heads()[0]), sorted(before),
                         "opening the other version changed the shared version graph")
        self.assertIsNotNone(self._banner(b), "B's banner went with nothing resolved")
        self._text_is(b, self.MINE, "nothing was merged")
        # Back on A's shared tab: still shared, still its own text, still two versions.
        self.focus_app(a)
        rawinput.keyCombo("<Control>Page_Up")
        self._text_is(a, self.MINE, "A's shared tab")
        self.wait_until(lambda: self._status(a), lambda name: bool(name),
                        description="A's shared tab to show its sharing status")
        self.assertIsNotNone(self._banner(a), "A's shared tab lost its banner with nothing resolved")
        self.assert_still_running(a, b)


class LettersCollabDefaultOff(_LettersAtspi, BaseGUITestCase):
    """Collaboration is off until the user acts: a build with the feature
    shows its menu items and has run nothing network-related."""

    app_name = "letters"

    def setUp(self):
        # The sidecar is findable (COLLABD_BIN), so an app that started it
        # eagerly would have it to start; without it the absence below would
        # prove nothing.
        self.prepare_instance_pair(labels=("a",))
        super().setUp()
        self.a = self.primary_instance()

    def _sidecars(self):
        return [row for row in instances.descendants(self.process.pid)
                if "collabd" in row["comm"] or "collabd" in row["cmdline"]]

    def test_default_off(self):
        self.assertEqual(self._sidecars(), [], "a sidecar runs before the user did anything")
        self.assertIsNone(self._status(self.a), "a sharing status shows before any sharing")

        self.new_letters_document()
        self.focus_app(self.a)
        rawinput.typeText("plain")
        self.wait_until(lambda: self._page_text(self.a),
                        lambda text: text.rstrip("\n") == "plain",
                        description="the page to hold 'plain'")

        # The menu offers sharing, and Stop Sharing is there but cannot be used.
        self._open_main_menu(self.a)
        share = self._menu_item(self.a, "Share This Document" + ELLIPSIS, need_action=False)
        join = self._menu_item(self.a, "Join a Shared Document" + ELLIPSIS, need_action=False)
        stop = self._menu_item(self.a, "Stop Sharing", need_action=False)
        self.assertTrue(self._is_sensitive(share), "Share This Document… is not available")
        self.assertTrue(self._is_sensitive(join), "Join a Shared Document… is not available")
        self.assertFalse(self._is_sensitive(stop), "Stop Sharing is available with nothing shared")
        self._close_main_menu(self.a)

        # A sidecar would start while the document opened, the menu showed and
        # the shell settled; give that its chance before looking.
        time.sleep(1.0)  # settling: a sidecar that started late must not go unseen
        self.assertEqual(self._sidecars(), [],
                         "a sidecar runs although nothing was shared or joined")
        self.assertIsNone(self._status(self.a), "a sharing status shows with nothing shared")
        self.assertIsNone(self.process.poll(), "letters exited")
