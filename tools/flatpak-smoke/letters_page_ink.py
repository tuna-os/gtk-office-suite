#!/usr/bin/env python3
"""The Letters Flatpak shows a document's text on the page.

    letters_page_ink.py BUNDLE.flatpak OUT_DIR [--no-deps]

The GUI smoke tests check that text is drawn on screen (#1692), but they
run the binary cargo built. The Flatpak, which is how Letters is installed,
is built in CI and never opened there: a blank page that only the sandbox
shows (its runtime's fonts, its GL stack, a portal that never answers)
would ship unseen.

This installs the bundle, opens a three-line letter in it under Xvfb, once
in the default style and once with the system preferring dark, and reads
the screen from Xvfb's framebuffer file (the builder image has no
screenshot tool and no Pillow). The page must hold the letter's ink within
a minute; a screenshot of each run is left in OUT_DIR either way.

--no-deps installs the bundle without fetching its runtime, for a machine
that has the runtime and no route to Flathub.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import zipfile

from xwd_ink import measure

APP_ID = "org.tunaos.letters"
# Three lines of 12pt text at 100% zoom: about 1400 ink pixels. A blank page
# holds only a caret (under 50), and one short line about 300.
MIN_INK = 600
TIMEOUT = 60.0
SCHEMES = {"default": None, "prefer-dark": "prefer-dark"}


def write_letter(path):
    w = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
    body = "".join(f"<w:p><w:r><w:t>{t}</w:t></w:r></w:p>" for t in (
        "Dear reader,",
        "This letter was opened in the Flatpak and its text must be drawn on the page.",
        "Yours sincerely,",
    ))
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("[Content_Types].xml",
                   '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
                   '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
                   '<Default Extension="xml" ContentType="application/xml"/>'
                   '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>')
        z.writestr("_rels/.rels",
                   '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
                   '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
        z.writestr("word/document.xml", f'<?xml version="1.0" encoding="UTF-8"?><w:document {w}><w:body>{body}</w:body></w:document>')


def run_once(name, scheme, letter, out_dir, display):
    """Open the letter in the Flatpak on its own Xvfb; the last measure."""
    fb = tempfile.mkdtemp(prefix=f"fb-{name}-")
    xvfb = subprocess.Popen(["Xvfb", f":{display}", "-screen", "0", "1280x900x24", "-fbdir", fb, "-nolisten", "tcp"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    env = {**os.environ, "DISPLAY": f":{display}"}
    env.pop("WAYLAND_DISPLAY", None)
    cmd = ["flatpak", "run", f"--filesystem={os.path.dirname(letter)}"]
    if scheme:
        cmd.append(f"--env=ADW_DEBUG_COLOR_SCHEME={scheme}")
    cmd += [APP_ID, letter]
    log = open(os.path.join(out_dir, f"{name}.log"), "w")
    app = None
    result = {"ink": 0, "page": None}
    try:
        time.sleep(2)
        # Its own session bus, as a desktop session would give it.
        app = subprocess.Popen(["dbus-run-session", "--"] + cmd, env=env, stdout=log, stderr=subprocess.STDOUT)
        screen = os.path.join(fb, "Xvfb_screen0")
        deadline = time.monotonic() + TIMEOUT
        while time.monotonic() < deadline:
            time.sleep(3)
            result = measure(screen)
            if result["ink"] >= MIN_INK or app.poll() is not None:
                break
        time.sleep(1)
        result = measure(screen, png=os.path.join(out_dir, f"{name}.png"))
        result["exited"] = app.poll()
    finally:
        if app and app.poll() is None:
            subprocess.run(["flatpak", "kill", APP_ID], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                app.wait(timeout=10)
            except subprocess.TimeoutExpired:
                app.kill()
        xvfb.terminate()
        xvfb.wait()
        shutil.rmtree(fb, ignore_errors=True)
        log.close()
    return result


def main():
    args = sys.argv[1:]
    no_deps = "--no-deps" in args
    args = [a for a in args if a != "--no-deps"]
    bundle, out_dir = args
    os.makedirs(out_dir, exist_ok=True)
    # Its locale extension is not needed to draw a letter.
    install = ["flatpak", "install", "--user", "-y", "--noninteractive", "--reinstall", "--no-related"]
    if no_deps:
        install.append("--no-deps")
    subprocess.run(install + [bundle], check=True)
    work = tempfile.mkdtemp(prefix="letters-flatpak-ink-")
    letter = os.path.join(work, "letter.docx")
    write_letter(letter)
    failed = []
    for display, (name, scheme) in enumerate(SCHEMES.items(), start=90):
        result = run_once(name, scheme, letter, out_dir, display)
        print(f"{name}: {json.dumps(result)}", flush=True)
        if result["ink"] < MIN_INK:
            failed.append(f"{name}: {result['ink']} ink pixels on the page (want {MIN_INK}+), "
                          f"page {result['page']}, app exit {result.get('exited')}")
    if failed:
        print("The Letters Flatpak did not show the letter's text:\n  " + "\n  ".join(failed)
              + f"\nScreenshots and logs: {out_dir}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
