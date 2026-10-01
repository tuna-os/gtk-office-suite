#!/usr/bin/env python3
"""After a capture, keep only the screenshots that visibly changed.

Two captures of the same UI are never byte-identical: font hinting differs
between runner images, and some shots contain the time (a tracked change's
timestamp, today's date in the smart-chip list, the rehearsal clock). Commit
those and every run proposes a refresh nobody should have to look at.

So every modified PNG under the given directories is compared with the
committed one. One whose differences are noise (fewer than NOISE_FRACTION
of its pixels changed by more than NOISE_LEVEL in any channel, at the same
size) is put back with `git checkout`. Anything else stays: a real UI
change, a new screenshot, a different size. The kept ones are listed, which
is what the pull request says changed.

Usage: keep_changed_screenshots.py <dir> [<dir> ...]
Exit status 0; prints "changed: <n>" last.
"""

import io
import subprocess
import sys

# A pixel counts as changed when any channel moved by more than this.
# Low on purpose: GNOME's surfaces differ by little (a sidebar's grey is
# about 15 from the window's), so the size of the change, not its depth,
# is what tells noise from a real change.
NOISE_LEVEL = 10
# ... and an image counts as changed when more than this fraction of its
# pixels did. A clock's digits are about 0.02% of a 960x680 window; a moved
# button, a new label or a different panel is well over 1%.
NOISE_FRACTION = 0.002


def changed_fraction(old, new):
    """Fraction of pixels that differ by more than NOISE_LEVEL, or None
    when the images differ in size (always a real change)."""
    from PIL import Image, ImageChops
    a, b = old.convert("RGB"), new.convert("RGB")
    if a.size != b.size:
        return None
    diff = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > NOISE_LEVEL else 0)
    return diff.histogram()[255] / (a.size[0] * a.size[1])


def is_noise(old, new):
    fraction = changed_fraction(old, new)
    return fraction is not None and fraction < NOISE_FRACTION


def git(*args, binary=False):
    out = subprocess.run(["git", *args], check=True, capture_output=True)
    return out.stdout if binary else out.stdout.decode()


def main(dirs):
    from PIL import Image
    modified = [p for p in git("diff", "--name-only", "--", *dirs).split() if p.endswith(".png")]
    kept = []
    for path in modified:
        old = Image.open(io.BytesIO(git("show", f"HEAD:{path}", binary=True)))
        new = Image.open(path)
        fraction = changed_fraction(old, new)
        if fraction is not None and fraction < NOISE_FRACTION:
            git("checkout", "--", path)
            print(f"noise    {path} ({fraction:.4%} of pixels)")
        else:
            kept.append(path)
            print(f"changed  {path} ({'new size' if fraction is None else f'{fraction:.2%} of pixels'})")
    added = [p for p in git("ls-files", "--others", "--exclude-standard", "--", *dirs).split() if p.endswith(".png")]
    deleted = [p for p in git("ls-files", "--deleted", "--", *dirs).split() if p.endswith(".png")]
    for path in added:
        print(f"new      {path}")
    for path in deleted:
        print(f"removed  {path}")
    print(f"changed: {len(kept) + len(added) + len(deleted)}")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    main(sys.argv[1:])
