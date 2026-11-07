#!/usr/bin/env python3
"""Drive the running Mandelbrot app through its accessibility tree (AT-SPI).

Used by tests/e2e/run-gui-call.sh. Wayland gives a client no way to inject
input into another, so everything goes through AT-SPI instead: entries are
filled with the EditableText interface and buttons are pressed with their
Action interface, the same calls a screen reader makes. That only works for
widgets that have an accessible name, which is also what this test checks.

Every step dumps the accessible tree to --dump-dir when it fails, so a red
run shows what the window actually contained.

Steps:
  login       homeserver page -> password login -> session setup
  join-call   press "Join Call" in the room header, then in the prescreen
  wait-name   wait until an accessible with the given name is showing
"""

import argparse
import sys
import time
from pathlib import Path

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib

APP_NAMES = ("mandelbrot", "fractal")


def log(message):
    print(f"[drive_app] {message}", flush=True)


def is_showing(node):
    try:
        states = node.get_state_set()
        return states.contains(Atspi.StateType.SHOWING) and states.contains(Atspi.StateType.VISIBLE)
    except GLib.Error:
        return False


def children(node):
    try:
        count = node.get_child_count()
    except GLib.Error:
        return []
    result = []
    for index in range(count):
        try:
            child = node.get_child_at_index(index)
        except GLib.Error:
            continue
        if child is not None:
            result.append(child)
    return result


def walk(node, depth=0):
    yield node, depth
    for child in children(node):
        yield from walk(child, depth + 1)


def describe(node):
    try:
        name = node.get_name() or ""
        role = node.get_role_name() or ""
        description = node.get_description() or ""
    except GLib.Error:
        return "<gone>"
    text = f"[{role}] {name!r}"
    if description:
        text += f" desc={description!r}"
    if is_showing(node):
        text += " (showing)"
    return text


def find_app(timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        desktop = Atspi.get_desktop(0)
        for app in children(desktop):
            name = (app.get_name() or "").lower()
            if any(candidate in name for candidate in APP_NAMES):
                return app
        time.sleep(0.5)
    desktop = Atspi.get_desktop(0)
    names = [app.get_name() for app in children(desktop)]
    raise SystemExit(f"app not on the accessibility bus after {timeout}s; apps: {names}")


class Driver:
    def __init__(self, app, dump_dir):
        self.app = app
        self.dump_dir = Path(dump_dir)
        self.dump_dir.mkdir(parents=True, exist_ok=True)
        self.dumps = 0

    def dump(self, label):
        self.dumps += 1
        path = self.dump_dir / f"{self.dumps:02d}-{label}.txt"
        with path.open("w") as file:
            for node, depth in walk(self.app):
                file.write("  " * depth + describe(node) + "\n")
        log(f"accessible tree dumped to {path}")
        return path

    def find_all(self, predicate):
        return [node for node, _ in walk(self.app) if is_showing(node) and predicate(node)]

    def wait_for(self, label, predicate, timeout):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            found = self.find_all(predicate)
            if found:
                return found[0]
            time.sleep(0.5)
        self.dump(f"missing-{label}")
        raise SystemExit(f"timed out after {timeout}s waiting for {label}")

    def wait_for_any(self, labelled_predicates, timeout):
        """Wait for the first of several (label, predicate); return (label, node)."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for label, predicate in labelled_predicates:
                found = self.find_all(predicate)
                if found:
                    return label, found[0]
            time.sleep(0.5)
        self.dump("missing-" + "-or-".join(label for label, _ in labelled_predicates))
        raise SystemExit(
            f"timed out after {timeout}s waiting for any of "
            f"{[label for label, _ in labelled_predicates]}"
        )

    def press(self, node, label):
        action = node.get_action_iface()
        if action is None:
            self.dump(f"no-action-{label}")
            raise SystemExit(f"{label}: {describe(node)} has no Action interface")
        names = [action.get_action_name(i) for i in range(action.get_n_actions())]
        index = next(
            (i for i, name in enumerate(names) if name in ("click", "activate", "press")), 0
        )
        log(f"pressing {label}: {describe(node)} via {names[index] if names else index!r}")
        action.do_action(index)

    def fill(self, node, text, label, secret=False):
        editable = node.get_editable_text_iface()
        if editable is None:
            self.dump(f"not-editable-{label}")
            raise SystemExit(f"{label}: {describe(node)} is not editable")
        log(f"filling {label}: {describe(node)} with {'***' if secret else text!r}")
        editable.set_text_contents(text)


def named(*names, roles=None):
    wanted = {name.lower() for name in names}

    def predicate(node):
        try:
            name = (node.get_name() or "").lower().replace("_", "")
            description = (node.get_description() or "").lower()
            role = node.get_role_name()
        except GLib.Error:
            return False
        if roles and role not in roles:
            return False
        return name in wanted or description in wanted

    return predicate


BUTTON = ("push button", "toggle button", "button")


def editable_in(driver, label_names):
    """The editable text widget of an entry row titled one of label_names.

    GTK names an EntryRow's inner text widget after the row title; fall back
    to the first editable inside a node with that name.
    """

    def predicate(node):
        return node.get_editable_text_iface() is not None and named(*label_names)(node)

    found = driver.find_all(predicate)
    if found:
        return found[0]
    for container in driver.find_all(named(*label_names)):
        for node, _ in walk(container):
            if node.get_editable_text_iface() is not None:
                return node
    return None


def wait_editable(driver, label_names, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        node = editable_in(driver, label_names)
        if node is not None:
            return node
        time.sleep(0.5)
    driver.dump("missing-entry-" + label_names[0].replace(" ", "-"))
    raise SystemExit(f"timed out waiting for the {label_names[0]!r} entry")


def is_checked(node):
    try:
        return node.get_state_set().contains(Atspi.StateType.CHECKED)
    except GLib.Error:
        return False


def step_login(driver, args):
    driver.press(driver.wait_for("log-in", named("Log In", roles=BUTTON), 60), "Log In")

    # A homeserver URL (rather than a server name) is only accepted with
    # auto-discovery off: Advanced… -> Auto-Discovery.
    driver.press(driver.wait_for("advanced", named("Advanced…", roles=BUTTON), 30), "Advanced…")
    switch = driver.wait_for("auto-discovery", named("Auto-Discovery"), 10)
    if is_checked(switch):
        driver.press(switch, "Auto-Discovery")
        time.sleep(1)
    if is_checked(switch):
        driver.dump("auto-discovery-still-on")
        raise SystemExit("could not turn auto-discovery off")
    close = driver.find_all(named("Close", roles=BUTTON))
    if close:
        driver.press(close[0], "Close")

    entry = wait_editable(driver, ("Homeserver", "Homeserver URL"), 30)
    driver.fill(entry, args.homeserver, "homeserver")
    driver.press(driver.wait_for("next", named("Next", roles=BUTTON), 10), "Next")

    user_entry = wait_editable(driver, ("Matrix Username",), 60)
    driver.fill(user_entry, args.username, "username")
    password_entry = wait_editable(driver, ("Password",), 10)
    driver.fill(password_entry, args.password, "password", secret=True)
    driver.press(driver.wait_for("next", named("Next", roles=BUTTON), 10), "Next")

    # Session setup for a brand-new account: create the crypto identity, then
    # account recovery, then acknowledge the recovery key. Each page is
    # optional (the app skips what is already done), so loop until the main
    # view shows up.
    deadline = time.monotonic() + args.timeout
    while time.monotonic() < deadline:
        label, node = driver.wait_for_any(
            [
                ("main-view", named(*args.done_names)),
                ("set-up", named("Set Up", roles=BUTTON)),
                ("done", named("Done", roles=BUTTON)),
            ],
            max(1, deadline - time.monotonic()),
        )
        if label == "main-view":
            log(f"logged in: found {describe(node)}")
            return
        driver.press(node, label)
        time.sleep(2)
    driver.dump("login-timeout")
    raise SystemExit("login did not reach the main view")


def step_join_call(driver, args):
    driver.press(
        driver.wait_for("join-call-header", named("Join Call", roles=BUTTON), args.timeout),
        "Join Call (header)",
    )

    # The header button opens the prescreen, a box labelled "Join Call" that
    # holds its own "Join Call" button. Press that one, not the header's again.
    def prescreen_button(node):
        if not named("Join Call", roles=BUTTON)(node):
            return False
        parent = node.get_parent()
        while parent is not None:
            if parent.get_role_name() not in BUTTON and named("Join Call")(parent):
                return True
            parent = parent.get_parent()
        return False

    driver.press(
        driver.wait_for("join-call-prescreen", prescreen_button, args.timeout),
        "Join Call (prescreen)",
    )
    driver.wait_for("leave-call", named("Leave Call", roles=BUTTON), args.timeout)
    log("in the call: 'Leave Call' is showing")


def step_leave_call(driver, args):
    driver.press(
        driver.wait_for("leave-call", named("Leave Call", roles=BUTTON), args.timeout), "Leave Call"
    )


def step_wait_name(driver, args):
    node = driver.wait_for(args.name, named(args.name), args.timeout)
    log(f"found {describe(node)}")


def main():
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--dump-dir", required=True)
    parser.add_argument("--timeout", type=float, default=120)
    sub = parser.add_subparsers(dest="step", required=True)
    login = sub.add_parser("login")
    login.add_argument("--homeserver", required=True)
    login.add_argument("--username", required=True)
    login.add_argument("--password", required=True)
    login.add_argument(
        "--done-names",
        nargs="+",
        default=["Main Menu"],
        help="accessible names that mean the main view is up",
    )
    sub.add_parser("join-call")
    sub.add_parser("leave-call")
    wait_name = sub.add_parser("wait-name")
    wait_name.add_argument("name")
    sub.add_parser("dump")
    args = parser.parse_args()

    Atspi.init()
    driver = Driver(find_app(60), args.dump_dir)
    log(f"attached to {describe(driver.app)}")
    if args.step == "login":
        step_login(driver, args)
    elif args.step == "join-call":
        step_join_call(driver, args)
    elif args.step == "leave-call":
        step_leave_call(driver, args)
    elif args.step == "wait-name":
        step_wait_name(driver, args)
    elif args.step == "dump":
        driver.dump("requested")
    return 0


if __name__ == "__main__":
    sys.exit(main())
