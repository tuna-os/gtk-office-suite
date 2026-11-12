#!/usr/bin/env bash
# shellcheck disable=SC2034 # the identity block is read through ${!name}
#
# Turn a checkout of Hylki into Lifafa, the TunaOS office suite's email client,
# in place.
#
#   cp -a . /tmp/lifafa && fork/rebrand.sh /tmp/lifafa
#   fork/rebrand.sh --identity      # print the identity below as KEY=value
#   HYLKI_REBRAND_FAST_PROFILE=1 fork/rebrand.sh <tree>   # see the manifest section
#
# The fork's source keeps upstream's co.hyprlab.Hylki strings so that merging
# hyprlab/hylki stays cheap; the rebrand is applied to a copy of the tree just
# before it is built (CI runs it on its own throwaway checkout). It rewrites:
#
#   - the app ID in the Rust constants, the desktop file, metainfo, D-Bus
#     service, icons and the Files extension, renaming every file named after
#     the ID, and the keyring service name;
#   - the display name (launcher, metainfo, window and About titles, the About
#     and welcome wordmark, tray);
#   - the config, data, cache and runtime directory names, so a native build
#     never shares state with upstream Hylki;
#   - the app icon, metainfo and project links, in the suite's style;
#   - the Flatpak manifest, written as fork/<APP_ID>.yml, which builds the
#     rebranded tree it sits in.
#
# Every rewrite names the text it expects. When upstream changes one of those
# sites the script stops with the file and the pattern, and a final sweep
# fails on any upstream ID or "hylki" directory join left behind, so drift in
# either direction is caught in CI rather than shipped.
#
# Running it again on a rebranded tree only re-runs that final sweep.
set -euo pipefail

# --- Identity ---------------------------------------------------------------
# Everything the app is called. A rename edits these lines and nothing else;
# fork-release.yml and import-from-hylki.sh read them through --identity.
APP_ID=org.tunaos.lifafa
APP_NAME=Lifafa
DATA_DIR_NAME=lifafa
KEYRING_SERVICE=org.tunaos.lifafa
DESKTOP_COMMENT='Email client — TunaOS office suite'
SUMMARY='Email client for the GNOME desktop'
DEVELOPER_NAME='Hanthor Contributors'
SUITE_NAME='Hanthor Office Suite'
HOMEPAGE=https://github.com/tuna-os/gtk-office-suite
BUGTRACKER=https://github.com/tuna-os/lifafa/issues
SOURCE_URL=https://github.com/tuna-os/lifafa
# The app icon; its 256 and 512 pixel renders sit beside it as -256.png and
# -512.png (rsvg-convert -w N -h N).
ICON=fork/icons/org.tunaos.lifafa.svg
# ---------------------------------------------------------------------------

IDENTITY=(APP_ID APP_NAME DATA_DIR_NAME KEYRING_SERVICE DESKTOP_COMMENT SUMMARY
    DEVELOPER_NAME SUITE_NAME HOMEPAGE BUGTRACKER SOURCE_URL ICON)
if [ "${1:-}" = --identity ]; then
    for name in "${IDENTITY[@]}"; do
        printf '%s=%s\n' "$name" "${!name}"
    done
    exit 0
fi

if [ $# -ne 1 ]; then
    echo "usage: $0 <tree>   (rewrites <tree> in place; run it on a copy)" >&2
    echo "       $0 --identity" >&2
    exit 2
fi
TREE="$(cd "$1" && pwd)"
HERE="$(cd "$(dirname "$0")" && pwd)"
# The script's own checkout is refused unless asked for: CI's checkout is
# disposable and says so, a developer's working tree is not.
if [ "$TREE" = "$(cd "$HERE/.." && pwd)" ] && [ -z "${HYLKI_REBRAND_IN_PLACE:-}" ]; then
    echo "refusing to rebrand this script's own checkout $TREE; copy it first" >&2
    echo "(or set HYLKI_REBRAND_IN_PLACE=1 on a throwaway checkout)" >&2
    exit 2
fi
for name in "${IDENTITY[@]}"; do
    export "${name?}"
done

exec python3 - "$TREE" <<'PY'
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from xml.sax.saxutils import escape

tree = Path(sys.argv[1])
os.chdir(tree)

OLD_ID = "co.hyprlab.Hylki"
APP_ID = os.environ["APP_ID"]
APP_NAME = os.environ["APP_NAME"]
DIR = os.environ["DATA_DIR_NAME"]
KEYRING = os.environ["KEYRING_SERVICE"]
SOURCE_URL = os.environ["SOURCE_URL"]
ICON = Path(os.environ["ICON"])
STAMP = Path("fork/.rebranded")
MANIFEST = Path(f"fork/{APP_ID}.yml")
# A name usable inside an identifier (the Files extension's menu item).
IDENT = re.sub(r"\W", "", APP_NAME)
x = escape


def fail(message):
    sys.exit(f"rebrand: {message}\n(upstream changed this site; update fork/rebrand.sh)")


def sub(path, pattern, replacement, count=None, flags=re.M):
    """Replace `pattern` in `path`; it must match, `count` times when given."""
    path = Path(path)
    if not path.is_file():
        fail(f"{path} is missing")
    text = path.read_text()
    new, n = re.subn(pattern, replacement, text, flags=flags)
    if n == 0 or (count is not None and n != count):
        want = "at least once" if count is None else f"{count} time(s)"
        fail(f"{path}: expected {pattern!r} {want}, found {n}")
    path.write_text(new)


def literal(text):
    """`text` as a regex replacement, with no group references in it."""
    return text.replace("\\", "\\\\")


def rename(old, new):
    old, new = Path(old), Path(new)
    if not old.exists():
        fail(f"{old} is missing")
    old.rename(new)


def install(source, target):
    if not Path(target).is_file():
        fail(f"{target} is missing")
    shutil.copyfile(source, target)


def text_files(*roots):
    for root in roots:
        for path in sorted(Path(root).rglob("*")):
            if not path.is_file():
                continue
            try:
                yield path, path.read_text()
            except UnicodeDecodeError:
                continue


# "Hylki" as a word in text a person reads, not part of an identifier
# ("HylkiTray", "X-Hylki-Icon-Launcher") or of the app ID.
USER_HYLKI = re.compile(r"(?<![\w.\-])Hylki(?![\w\-])")
# Strings that name Hylki on purpose: the credits this script writes into the
# About window, which are matched as whole literals.
CREDIT_FOOTER = "Built on Hylki by hyprlab. © 2026 Hyprlab"
CREDIT_COFFEE = "Buy Hylki's author a coffee"
# Upstream's own history, left as it is: the carry-over from Vireo and Veem
# (and its "Welcome to Hylki, formerly …" notice).
KEEP_FILES = {"src/desktop/legacy.rs", "src/ui/carry_over.rs"}
# Names nobody reads: the private icon theme.
KEEP_LITERALS = {("src/desktop/icon_theme.rs", "Hylki"), *((None, c) for c in (CREDIT_FOOTER, CREDIT_COFFEE))}


def rust_literals(src):
    """The (start, end) of every string literal's contents in Rust source,
    skipping comments and char literals."""
    i, n = 0, len(src)
    while i < n:
        if src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
        elif src.startswith("/*", i):
            depth = 0
            while i < n:
                if src.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif src.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                    if depth == 0:
                        break
                else:
                    i += 1
        elif (raw := re.match(r'b?r(#*)"', src[i:i + 300])) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            start = i + raw.end()
            end = src.find('"' + raw.group(1), start)
            yield start, end
            i = end + 1 + len(raw.group(1))
        elif src[i] == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            yield i + 1, j
            i = j + 1
        elif src[i] == "'":
            char = re.match(r"'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^\\'])'", src[i:i + 16])
            i += char.end() if char else 1
        elif src[i].isalnum() or src[i] == "_":
            i += re.match(r"\w+", src[i:i + 300]).end()
        else:
            i += 1


def user_hylki(path, text):
    """Where a Rust file's string literals call the app Hylki."""
    if str(path) in KEEP_FILES:
        return
    for start, end in rust_literals(text):
        literal_text = text[start:end]
        if "/co/hyprlab/Hylki" in literal_text or (None, literal_text) in KEEP_LITERALS \
                or (str(path), literal_text) in KEEP_LITERALS:
            continue
        for m in USER_HYLKI.finditer(literal_text):
            yield start + m.start(), start + m.end()


# Where a leftover would change what runs or what gets installed.
SWEPT = ("src", "data", "resources")


def sweep_check():
    leftovers = []
    for path, text in text_files(*SWEPT):
        if OLD_ID in text:
            leftovers.append(f"{path}: {OLD_ID}")
        if path.suffix == ".rs" and re.search(r'\.join\("hylki[/"]', text):
            leftovers.append(f'{path}: .join("hylki…")')
    for name in ("build.rs", MANIFEST):
        if OLD_ID in Path(name).read_text():
            leftovers.append(f"{name}: {OLD_ID}")
    for root in SWEPT:
        for path in Path(root).rglob("*" + OLD_ID + "*"):
            leftovers.append(f"{path}: file name")
    for path, text in text_files("src"):
        if path.suffix == ".rs":
            for start, _ in user_hylki(path, text):
                leftovers.append(f"{path}:{text.count(chr(10), 0, start) + 1}: Hylki in a string")
    if leftovers:
        fail("upstream names left after the rebrand:\n  " + "\n  ".join(leftovers))


if STAMP.exists():
    sweep_check()
    print(f"rebrand: {tree} is already {APP_NAME}")
    sys.exit(0)

icon_png = {size: ICON.with_name(f"{ICON.stem}-{size}.png") for size in (256, 512)}
icon_symbolic = ICON.with_name(f"{ICON.stem}-symbolic.svg")
# Settings → App icon offers these beside the default, by upstream's file names.
icon_alt = ICON.parent / "alt"
for path in (ICON, icon_symbolic, *icon_png.values()):
    if not path.is_file():
        sys.exit(f"rebrand: the icon {path} is missing")

# --- Files named after the app ID -------------------------------------------
expected_files = [
    "data/co.hyprlab.Hylki.desktop",
    "data/co.hyprlab.Hylki.metainfo.xml",
    "data/co.hyprlab.Hylki.Restart.service",
    "data/co.hyprlab.Hylki.search-provider.ini",
    "data/co.hyprlab.Hylki.service",
    "data/icons/hicolor/256x256/apps/co.hyprlab.Hylki.png",
    "data/icons/hicolor/512x512/apps/co.hyprlab.Hylki.png",
    "data/icons/hicolor/scalable/apps/co.hyprlab.Hylki.svg",
    "data/icons/hicolor/symbolic/apps/co.hyprlab.Hylki-symbolic.svg",
    "resources/icons/scalable/apps/co.hyprlab.Hylki.svg",
]
for name in expected_files:
    if not Path(name).is_file():
        fail(f"{name} is missing")
for root in SWEPT:
    for path in sorted(Path(root).rglob("*" + OLD_ID + "*")):
        rename(path, path.with_name(path.name.replace(OLD_ID, APP_ID)))

# The GNOME Files extension is installed on the host beside upstream's, so it
# needs a file name, a load marker and a menu item name of its own.
nautilus = f"data/nautilus/{DIR}-nautilus.py"
rename("data/nautilus/hylki-nautilus.py", nautilus)
sub(
    "src/desktop/nautilus_ext.rs",
    r'"\.\./\.\./data/nautilus/hylki-nautilus\.py"',
    literal(f'"../../{nautilus}"'),
    count=1,
)
sub("src/desktop/nautilus_ext.rs", r'FILE_NAME: &str = "hylki-nautilus\.py"',
    literal(f'FILE_NAME: &str = "{DIR}-nautilus.py"'), count=1)
sub("src/desktop/nautilus_ext.rs", r'"\.hylki-nautilus\.loaded"',
    literal(f'".{DIR}-nautilus.loaded"'), count=1)
sub(nautilus, r'"\.hylki-nautilus\.loaded"', literal(f'".{DIR}-nautilus.loaded"'), count=1)
sub(nautilus, r'"hylki", "launcher"', literal(f'"{DIR}", "launcher"'), count=1)
sub(nautilus, r'name="HylkiMenuProvider::send"', literal(f'name="{IDENT}MenuProvider::send"'), count=1)
sub(nautilus, r'^(\s*"(?:label|tip)": ".*)\bHylki\b', lambda m: m.group(1) + APP_NAME)

# --- The app ID, everywhere it is read at run time or installed -------------
# Required sites first, so a renamed constant fails here instead of slipping
# past the sweep.
sub("src/main.rs", r'^    "co\.hyprlab\.Hylki"$', literal(f'    "{APP_ID}"'), count=1)
sub("src/main.rs", r'^    "co\.hyprlab\.Hylki\.Beta"$', literal(f'    "{APP_ID}.Beta"'), count=1)
# Credentials live under their own Secret Service name, apart from upstream's.
sub("src/config/secrets.rs", r'KEYRING_SERVICE: &str = "co\.hyprlab\.Hylki";',
    literal(f'KEYRING_SERVICE: &str = "{KEYRING}";'), count=1)
sub("src/config/snapshot_tests.rs", r'assert_eq!\(KEYRING_SERVICE, "co\.hyprlab\.Hylki"\);',
    literal(f'assert_eq!(KEYRING_SERVICE, "{KEYRING}");'), count=1)
search_ini = f"data/{APP_ID}.search-provider.ini"
sub(search_ini, r"^DesktopId=co\.hyprlab\.Hylki\.desktop$", literal(f"DesktopId={APP_ID}.desktop"), count=1)
sub(search_ini, r"^BusName=co\.hyprlab\.Hylki$", literal(f"BusName={APP_ID}"), count=1)
activation = f"data/{APP_ID}.service"
sub(activation, r"^Name=co\.hyprlab\.Hylki$", literal(f"Name={APP_ID}"), count=1)
service = f"data/{APP_ID}.Restart.service"
sub(service, r"^Name=co\.hyprlab\.Hylki\.Restart$", literal(f"Name={APP_ID}.Restart"), count=1)
desktop = f"data/{APP_ID}.desktop"
sub(desktop, r"^Icon=co\.hyprlab\.Hylki$", literal(f"Icon={APP_ID}"), count=1)
sub(desktop, r"^StartupWMClass=co\.hyprlab\.Hylki$", literal(f"StartupWMClass={APP_ID}"), count=1)
metainfo = f"data/{APP_ID}.metainfo.xml"
sub(metainfo, r"<id>co\.hyprlab\.Hylki</id>", literal(f"<id>{APP_ID}</id>"), count=1)
sub("resources/hylki.gresource.xml", r"icons/scalable/apps/co\.hyprlab\.Hylki\.svg",
    literal(f"icons/scalable/apps/{APP_ID}.svg"), count=1)
# Then every other mention: icon includes, test expectations, comments.
for path, text in text_files(*SWEPT):
    if OLD_ID in text:
        path.write_text(text.replace(OLD_ID, APP_ID))
# The bundled icons' resource prefix (/co/hyprlab/Hylki) is left alone: it is
# private to the process, and main.rs pins the base path to it explicitly.

# --- The app icon -----------------------------------------------------------
# Everywhere the app's own icon is installed or embedded: the launcher's
# hicolor icons, the in-app icon resource, the PNG the app embeds for
# Settings → App icon and the sign-in page, the symbolic icon the tray draws,
# and the alternatives Settings → App icon offers.
install(ICON, f"data/icons/hicolor/scalable/apps/{APP_ID}.svg")
install(ICON, f"resources/icons/scalable/apps/{APP_ID}.svg")
for size, png in icon_png.items():
    install(png, f"data/icons/hicolor/{size}x{size}/apps/{APP_ID}.png")
install(icon_symbolic, f"data/icons/hicolor/symbolic/apps/{APP_ID}-symbolic.svg")
upstream_alt = sorted(p.name for p in Path("data/icons/alt").glob("*.png"))
if not upstream_alt:
    fail("data/icons/alt has no alternative icons")
for name in upstream_alt:
    if not (icon_alt / name).is_file():
        sys.exit(f"rebrand: upstream offers data/icons/alt/{name}; draw {icon_alt / name} to match")
    install(icon_alt / name, f"data/icons/alt/{name}")

# --- The display name -------------------------------------------------------
sub(
    "src/main.rs",
    r'(const APP_NAME: &str = if cfg!\(feature = "beta"\) \{\n    )"Hylki \(beta\)"(\n\} else \{\n    )"Hylki"',
    lambda m: f'{m.group(1)}"{APP_NAME} (beta)"{m.group(2)}"{APP_NAME}"',
    count=1,
)
sub("src/main.rs", r'set_application_name\("Hylki"\)', literal(f'set_application_name("{APP_NAME}")'), count=1)
sub("src/app/tray/indicator.rs", r'(fn title\(&self\) -> String \{\n\s*)"Hylki"',
    lambda m: f'{m.group(1)}"{APP_NAME}"', count=1)
# The wordmark the About window and the welcome wizard show is upstream's
# "Hylki" lettering beside its icon, as an SVG on a 128x42 box. Ours is the
# name in Cantarell beside the app icon, drawn by librsvg like theirs.
icon_body = re.sub(r"\A\s*<svg[^>]*>|</svg>\s*\Z", "", ICON.read_text())
for variant, color in (("black", "#1b1e32"), ("white", "#ffffff")):
    path = Path(f"data/about/wordmark-{variant}.svg")
    if not path.is_file():
        fail(f"{path} is missing")
    path.write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="128" height="42" viewBox="0 0 128 42">\n'
        f'  <text x="1" y="30" font-family="Cantarell" font-weight="bold" font-size="25" fill="{color}">'
        f"{x(APP_NAME)}</text>\n"
        f'  <svg x="86" y="0" width="42" height="42" viewBox="0 0 128 128">{icon_body}</svg>\n'
        "</svg>\n"
    )
desktop_comment = os.environ["DESKTOP_COMMENT"]
sub(desktop, r"^Name=Hylki$", literal(f"Name={APP_NAME}"), count=1)
sub(desktop, r"^Comment=.*$", literal(f"Comment={desktop_comment}"), count=1)
# Not a rename of Vireo or Veem: those are upstream's own predecessors.
sub(desktop, r"^# Earlier names.*\nX-Flatpak-RenamedFrom=.*\n", "", count=1)

# --- Metainfo, in the office suite's style ----------------------------------
# The app's description names it by its own name; the releases stay as
# upstream wrote them, since they are the history of the code being built
# (and some of them are about Hylki's own renames).
sub(metainfo, r"^  <!-- Hylki was called .*-->\n  <replaces>\n(?:    <id>.*</id>\n)+  </replaces>\n", "", count=1)
sub(metainfo, r"^  <name>Hylki</name>$", literal(f"  <name>{x(APP_NAME)}</name>"), count=1)
sub(metainfo, r"^  <summary>.*</summary>$", literal(f"  <summary>{x(os.environ['SUMMARY'])}</summary>"), count=1)
sub(metainfo, r"^  <description>\n(?:.*\n)*?  </description>\n",
    lambda m: USER_HYLKI.sub(lambda _: x(APP_NAME), m.group(0)), count=1)
sub(
    metainfo,
    r"^(  <description>\n)",
    lambda m: m.group(1)
    + f"    <p>{x(APP_NAME)} is an email client for the GNOME desktop, part of the "
    f"{x(os.environ['SUITE_NAME'])}.</p>\n"
    # No scheme on the link: AppStream warns about plain-text URLs here.
    "    <p>It is built on Hylki by hyprlab (github.com/hyprlab/hylki), with "
    "native Tuta and Proton Mail accounts added.</p>\n",
    count=1,
)
sub(metainfo, r'<url type="homepage">https://hylki\.hyprlab\.co</url>',
    literal(f'<url type="homepage">{x(os.environ["HOMEPAGE"])}</url>'), count=1)
sub(metainfo, r'<url type="bugtracker">https://github\.com/hyprlab/hylki/issues</url>',
    literal(f'<url type="bugtracker">{x(os.environ["BUGTRACKER"])}</url>'), count=1)
sub(metainfo, r'<url type="vcs-browser">https://github\.com/hyprlab/hylki</url>',
    literal(f'<url type="vcs-browser">{x(SOURCE_URL)}</url>'), count=1)
sub(metainfo, r'<developer id="co\.hyprlab">\n    <name>Hyprlab</name>\n  </developer>\n  <update_contact>.*</update_contact>',
    literal(f"<developer_name>{x(os.environ['DEVELOPER_NAME'])}</developer_name>"), count=1)
sub(metainfo, r"raw\.githubusercontent\.com/hyprlab/hylki/", "raw.githubusercontent.com/tuna-os/lifafa/")

# --- Directories ------------------------------------------------------------
# Config, data and cache all join "hylki" onto an XDG base; the MCP socket
# joins it onto the runtime directory outside Flatpak.
required_joins = [
    "src/config.rs",
    "src/config/accounts.rs",
    "src/cache/open.rs",
    "src/cache/bodies.rs",
    "src/desktop/app_icon.rs",
    "src/mcp.rs",
]
joins = re.compile(r'\.join\("hylki([/"])')
for name in required_joins:
    if not joins.search(Path(name).read_text()):
        fail(f'{name}: expected a .join("hylki…") directory')
for path, text in text_files("src"):
    if path.suffix == ".rs" and joins.search(text):
        path.write_text(joins.sub(lambda m: f'.join("{DIR}{m.group(1)}', text))
# The name an assistant's MCP client registers the server under.
sub("src/mcp.rs", r'^    "hylki-beta"\n\} else \{\n    "hylki"$',
    literal(f'    "{DIR}-beta"\n}} else {{\n    "{DIR}"'), count=1)

# --- Project links ----------------------------------------------------------
about = "src/app/about.rs"
sub(about, r'"https://hylki\.hyprlab\.co"', literal(f'"{os.environ["HOMEPAGE"]}"'), count=1)
sub(about, r'"https://github\.com/hyprlab/hylki/issues"', literal(f'"{os.environ["BUGTRACKER"]}"'), count=1)
sub(about, r'"https://github\.com/hyprlab/hylki",', literal(f'"{SOURCE_URL}",'), count=1)
sub("src/ui/welcome/pages.rs", r'"url", "https://github\.com/hyprlab/hylki"',
    literal(f'"url", "{os.environ["BUGTRACKER"]}"'), count=1)
# Upstream's community channels are hyprlab's, not this app's: questions go
# to the bug tracker above.
sub(about, r'^    links\.append\(&mk_row\("Discord", "https://discord\.gg/[^"]*"\)\);\n', "", count=1)
sub(about, r'^    links\.append\(&mk_row\("hyprlab@proton\.me", "mailto:hyprlab@proton\.me"\)\);\n', "", count=1)

# --- Every string that calls the app Hylki ----------------------------------
# Window and dialog text, notifications, the tray, the MCP tools' text, the
# sign-in page, and the fixtures the tests compare them with: every string
# literal in src/ that names Hylki names the app instead. Identifiers, the
# resource prefix and environment variables never match (see USER_HYLKI),
# and the files in KEEP_FILES are upstream's own history. Their translations
# no longer match and fall back to English.
expected_strings = {
    "src/ui/welcome.rs": "Welcome to Hylki",
    "src/app/palette.rs": "Quit Hylki entirely",
    "src/app/tray/indicator.rs": "Open Hylki",
    "src/app/hand_off.rs": "Send {n} files with Hylki",
    "src/ui/preferences/pages/privacy.rs": "Hylki collects no telemetry",
    "src/contacts/contact_sources.rs": "Hylki Address Book",
    "src/services/oauth/flow.rs": "<title>Hylki",
}
for name, needle in expected_strings.items():
    if needle not in Path(name).read_text():
        fail(f"{name}: expected the string {needle!r}")
renamed = 0
for path, text in text_files("src"):
    if path.suffix != ".rs":
        continue
    spans = list(user_hylki(path, text))
    for start, end in reversed(spans):
        text = text[:start] + APP_NAME + text[end:]
    if spans:
        path.write_text(text)
        renamed += len(spans)
if renamed < 150:
    fail(f"expected upward of 150 strings naming Hylki in src/, found {renamed}")
# Golden files the tests compare those strings with.
for path in sorted(Path("src").rglob("testdata/**/*")):
    if path.is_file():
        try:
            text = path.read_text()
        except UnicodeDecodeError:
            continue
        if USER_HYLKI.search(text):
            path.write_text(USER_HYLKI.sub(lambda _: APP_NAME, text))
# The credits, in the About window: who the app is built on, and that the
# coffee goes to them.
sub(about, r'i18n\("© 2026 Hyprlab"\)', literal(f'i18n("{CREDIT_FOOTER}")'), count=1)
sub(about, r'\.title\(i18n\("Buy Me a Coffee"\)\)', literal(f'.title(i18n("{CREDIT_COFFEE}"))'), count=1)

# --- The app's own defaults -------------------------------------------------
# Sender pictures are on for a new install: Gravatar portraits and the
# senders' site icons. Upstream keeps both off. An existing privacy.toml keeps
# what it says; a file from before these keys reads them as on too. The
# golden files the snapshot tests compare the defaults with follow.
privacy = "src/config/privacy.rs"
for field in ("gravatar", "sender_logos"):
    sub(privacy, rf"^    #\[serde\(default\)\]\n    pub\(crate\) {field}: bool,$",
        literal(f'    #[serde(default = "default_on")]\n    pub(crate) {field}: bool,'), count=1)
    sub(privacy, rf"^            {field}: false,$", literal(f"            {field}: true,"), count=1)
    sub("src/ui/message_list/init.rs", rf"^            {field}: false,$",
        literal(f"            {field}: true,"), count=1)
sub(privacy, r"^    /// Whether to load sender avatars from Gravatar \(off by default — it sends\n"
    r"    /// a hash of each sender's email to a third party\)\.$",
    literal("    /// Whether to load sender avatars from Gravatar (on by default in this app;\n"
            "    /// it sends a hash of each sender's email to a third party)."), count=1)
for golden in ("privacy-default.toml", "privacy-empty-file.toml", "bundle.toml"):
    sub(f"src/config/testdata/{golden}",
        r"^remote_ahead = false\ngravatar = false\navatars = true\nsender_logos = false$",
        literal("remote_ahead = false\ngravatar = true\navatars = true\nsender_logos = true"),
        count=1)

# --- The Flatpak manifest ---------------------------------------------------
# Upstream's manifest pins a tagged commit of hyprlab/hylki. A pinned source
# would be built without this rebrand, so the fork's manifest builds the tree
# it sits in instead: tools/local-manifest.sh makes that conversion, and its
# absolute paths are turned into ones relative to fork/.
manifest = Path("co.hyprlab.Hylki.yml").read_text()
manifest, n = re.subn(
    r"\A(?:#.*\n)+",
    lambda m: f"# Flatpak manifest for {APP_NAME} ({APP_ID}), the TunaOS office suite's\n"
    f"# email client, built on Hylki from {SOURCE_URL}. Generated by\n"
    "# fork/rebrand.sh from upstream's manifest; it builds the rebranded tree it\n"
    "# sits in (fork/README.md).\n",
    manifest,
)
if n != 1:
    fail("co.hyprlab.Hylki.yml: expected a leading comment block")
if not re.search(r"^id: co\.hyprlab\.Hylki$", manifest, re.M):
    fail("co.hyprlab.Hylki.yml: expected `id: co.hyprlab.Hylki`")
for needed in ("  - name: hylki-tuta\n", "  - name: hylki-proton\n", "org.freedesktop.Sdk.Extension.golang"):
    if needed not in manifest:
        fail(f"co.hyprlab.Hylki.yml: expected {needed.strip()!r}")
manifest = manifest.replace(OLD_ID, APP_ID)
staged = Path(f"{APP_ID}.rebrand.yml")
staged.write_text(manifest)
try:
    subprocess.run(["tools/local-manifest.sh", str(MANIFEST.resolve()), str(staged.resolve())],
                   check=True, stdout=subprocess.DEVNULL)
finally:
    staged.unlink()
manifest = MANIFEST.read_text()
root = str(tree)
if f"        path: {root}\n" not in manifest:
    fail(f"{MANIFEST}: tools/local-manifest.sh did not point the sources at {root}")
manifest = manifest.replace(f"        path: {root}\n", "        path: ..\n")
manifest = manifest.replace(f"      - {root}/", "      - ../")
if root in manifest:
    fail(f"{MANIFEST}: an absolute path to {root} is left")
MANIFEST.write_text(manifest)

# The Rust modules build in a cargo target directory under /run/ccache, which
# flatpak-builder mounts from its state directory (--ccache), so a build that
# keeps that directory, as fork-release.yml does, recompiles only the crates
# that changed. The binaries are installed from there. Each module first puts
# back the timestamps flatpak-builder's copy loses (fork/ci-mtimes.py); in a
# build that did not record them, that does nothing.
for binary in ("tuta-helper", "hylki"):
    sub(MANIFEST, rf"^      - install -Dm755 target/release/{binary} ",
        literal(f'      - install -Dm755 "$CARGO_TARGET_DIR"/release/{binary} '), count=1)
sub(MANIFEST, r"^        CARGO_NET_OFFLINE: 'true'\n",
    literal("        CARGO_NET_OFFLINE: 'true'\n"
            "        CARGO_TARGET_DIR: /run/ccache/cargo-target/hylki-tuta\n"), count=1)
# HYLKI_REBRAND_FAST_PROFILE=1 builds the app without LTO and with 16 codegen
# units: a change to the app crate then recompiles in a fraction of the time
# upstream's release profile (thin LTO, one codegen unit) takes, for a binary
# that is somewhat larger and slower. fork-release.yml asks for it on manual
# runs that publish nothing; tags and the daily build of main keep upstream's
# profile. It gets a target directory of its own, since every crate built
# under one profile is stale under the other.
app_env = "        CARGO_TARGET_DIR: /run/ccache/cargo-target/hylki\n"
if os.environ.get("HYLKI_REBRAND_FAST_PROFILE"):
    app_env = ("        CARGO_TARGET_DIR: /run/ccache/cargo-target/hylki-fast\n"
               "        CARGO_PROFILE_RELEASE_LTO: 'off'\n"
               "        CARGO_PROFILE_RELEASE_CODEGEN_UNITS: '16'\n")
sub(MANIFEST, r"^  - name: hylki\n    buildsystem: simple\n",
    literal("  - name: hylki\n    buildsystem: simple\n    build-options:\n      env:\n" + app_env), count=1)
APPLY_MTIMES = ('      - python3 "$FLATPAK_BUILDER_BUILDDIR"/fork/ci-mtimes.py apply'
                ' "$FLATPAK_BUILDER_BUILDDIR" "$CARGO_HOME"/vendor\n')
for first in ("cargo --offline build --locked --release --verbose -p tuta-helper",
              "cargo --offline fetch --manifest-path Cargo.toml --verbose"):
    sub(MANIFEST, rf"^    build-commands:\n      - {re.escape(first)}\n",
        literal(f"    build-commands:\n{APPLY_MTIMES}      - {first}\n"), count=1)

sweep_check()
STAMP.write_text(f"{APP_ID}\n")
print(f"rebrand: {tree} is now {APP_NAME} ({APP_ID})")
PY
