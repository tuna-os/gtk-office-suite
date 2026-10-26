"""Pure helpers for journeys that run two instances of one app.

Kept free of GTK, AT-SPI, dogtail and imaging imports, like
`owned_processes.py`, so what they decide can be asserted in the plain-Python
test lane with no display (tests/test_gui_harness.py). `base.py` does the
launching and the xdotool calls; the questions that have an answer without a
display live here:

* which environment keeps one instance's settings, data, state and sidecar
  identity away from the other's (`instance_env`);
* where the sidecar binary is (`resolve_collabd`);
* what the process tree under an app looks like (`descendants`), for "nothing
  network-related ran" without reading the app's own bookkeeping;
* where two windows go on one screen (`side_by_side`) and whether they do not
  overlap (`rects_disjoint`);
* what an AT-SPI text run says about a character (`text_runs`).
"""

import os

XDG_DIRS = ("config", "data", "cache", "state")


def instance_env(root, collabd_bin=None, create=True):
    """The environment of one instance, rooted at the directory `root`.

    Every store an instance writes sits under `root`, so two instances share
    nothing but the display and the session bus:

    * XDG_CONFIG_HOME / XDG_DATA_HOME / XDG_CACHE_HOME / XDG_STATE_HOME: the
      settings, documents, autosave and the like;
    * GSETTINGS_BACKEND=keyfile, which keeps its file under that
      XDG_CONFIG_HOME (the default dconf store is a per-user daemon a private
      directory does not reach);
    * COLLABD_DATA_DIR: the sidecar's data directory, which holds its Durable
      Streams identity key, so two instances are two different people;
    * COLLABD_BIN, when given: where the shell finds the sidecar;
    * GTK_OFFICE_TEST_MODE and GTK_OFFICE_NON_UNIQUE: both, because the apps
      add `ApplicationFlags::NON_UNIQUE` only when both are set, which is what
      lets a second process on the same bus start instead of forwarding to the
      first (suite-common `SuiteApp::new`).
    """
    env = {
        f"XDG_{name.upper()}_HOME": os.path.join(root, name) for name in XDG_DIRS
    }
    env["GSETTINGS_BACKEND"] = "keyfile"
    env["COLLABD_DATA_DIR"] = os.path.join(root, "collabd")
    if collabd_bin:
        env["COLLABD_BIN"] = collabd_bin
    env["GTK_OFFICE_TEST_MODE"] = "1"
    env["GTK_OFFICE_NON_UNIQUE"] = "1"
    if create:
        for path in [env[f"XDG_{name.upper()}_HOME"] for name in XDG_DIRS] + [
            env["COLLABD_DATA_DIR"]
        ]:
            os.makedirs(path, exist_ok=True)
    return env


def resolve_collabd(environ, workspace_dir):
    """Where the sidecar binary is: COLLABD_BIN, else the collab workspace's
    own debug build (`cd collab && cargo build -p collabd`)."""
    explicit = environ.get("COLLABD_BIN")
    if explicit:
        return explicit
    return os.path.join(workspace_dir, "collab", "target", "debug", "collabd")


def _stat_fields(text):
    """(pid, comm, ppid) from the text of a /proc/<pid>/stat.

    `comm` sits between parentheses and may itself hold spaces or
    parentheses, so the fields are split after the *last* `)`."""
    head, _, rest = text.rpartition(")")
    pid, _, comm = head.partition("(")
    return int(pid), comm, int(rest.split()[1])


def descendants(pid, proc_root="/proc"):
    """Every live process below `pid` (children, their children, ...), as
    dicts with `pid`, `ppid`, `comm` and `cmdline` (argv joined by spaces).

    Read from /proc/<pid>/stat rather than `/proc/<pid>/task/*/children`,
    which exists only on kernels built with CONFIG_PROC_CHILDREN."""
    rows = {}
    for name in os.listdir(proc_root):
        if not name.isdigit():
            continue
        try:
            with open(os.path.join(proc_root, name, "stat"), encoding="utf-8",
                      errors="replace") as handle:
                child, comm, ppid = _stat_fields(handle.read())
        except (OSError, ValueError, IndexError):
            continue  # exited while we were looking, or not a process
        try:
            with open(os.path.join(proc_root, name, "cmdline"), "rb") as handle:
                cmdline = handle.read().replace(b"\0", b" ").decode(
                    "utf-8", "replace").strip()
        except OSError:
            cmdline = ""
        rows[child] = {"pid": child, "ppid": ppid, "comm": comm, "cmdline": cmdline}
    found = []
    frontier = [pid]
    while frontier:
        parent = frontier.pop()
        for row in rows.values():
            if row["ppid"] == parent:
                found.append(row)
                frontier.append(row["pid"])
    return sorted(found, key=lambda row: row["pid"])


def side_by_side(screen_width, screen_height, count):
    """`count` equal columns across the screen, as (x, y, width, height)."""
    if count < 1:
        raise ValueError("count must be at least 1")
    width = screen_width // count
    return [(index * width, 0, width, screen_height) for index in range(count)]


def rects_disjoint(rects):
    """Whether no two (x, y, width, height) rectangles overlap."""
    for i, (ax, ay, aw, ah) in enumerate(rects):
        for bx, by, bw, bh in rects[i + 1:]:
            if ax < bx + bw and bx < ax + aw and ay < by + bh and by < ay + ah:
                return False
    return True


def parse_shell_geometry(text):
    """The x, y, width, height of `xdotool getwindowgeometry --shell`:
    lines `WINDOW=..`, `X=..`, `Y=..`, `WIDTH=..`, `HEIGHT=..`."""
    values = {}
    for line in text.splitlines():
        key, sep, value = line.partition("=")
        if sep and value.strip().lstrip("-").isdigit():
            values[key.strip()] = int(value)
    missing = [k for k in ("X", "Y", "WIDTH", "HEIGHT") if k not in values]
    if missing:
        raise ValueError(f"no {', '.join(missing)} in geometry output {text!r}")
    return values["X"], values["Y"], values["WIDTH"], values["HEIGHT"]


def attributes_dict(raw):
    """AT-SPI text attributes as a dict, whichever shape the binding returned:
    a mapping already, or the `name:value;name:value` string some bindings give
    (a `\\` escapes `:` and `;` inside a name or a value)."""
    if isinstance(raw, dict):
        return {str(k): str(v) for k, v in raw.items()}
    attrs = {}
    for item in _split_unescaped(str(raw or ""), ";"):
        key, sep, value = _partition_unescaped(item, ":")
        if sep and key.strip():
            attrs[_unescape(key.strip())] = _unescape(value.strip())
    return attrs


def _split_unescaped(text, sep):
    parts, current, escaped = [], [], False
    for char in text:
        if escaped:
            current.append("\\" + char)
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == sep:
            parts.append("".join(current))
            current = []
        else:
            current.append(char)
    parts.append("".join(current))
    return [p for p in parts if p]


def _partition_unescaped(text, sep):
    escaped = False
    for index, char in enumerate(text):
        if escaped:
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == sep:
            return text[:index], sep, text[index + 1:]
    return text, "", ""


def _unescape(text):
    out, escaped = [], False
    for char in text:
        if escaped:
            out.append(char)
            escaped = False
        elif char == "\\":
            escaped = True
        else:
            out.append(char)
    return "".join(out)


def text_runs(get_attributes, length):
    """The attribute runs covering offsets 0..length, as a list of
    (start, end, attrs), from `get_attributes(offset) -> (attrs, start, end)`
    (AT-SPI's `Text.get_attributes`).

    Each run is clipped to the text and asked for again at its `end`; a binding
    that answers with a run that does not advance past the offset it was asked
    about still moves on by one character, so a bad answer cannot loop."""
    runs = []
    offset = 0
    while offset < length:
        attrs, start, end = get_attributes(offset)
        end = max(end, offset + 1)
        runs.append((min(start, offset), min(end, length), attributes_dict(attrs)))
        offset = end
    return runs
