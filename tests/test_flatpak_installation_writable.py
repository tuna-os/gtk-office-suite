"""Every job that builds with flatpak-builder in the flathub-infra image first
copies the image's Flatpak installation into the container's writable layer.

The image ships its SDK in /var/lib/flatpak, in its own layers. When Flathub
has a newer SDK, flatpak-builder updates it in place, and on some runners
that fails with "Invalid cross-device link" before anything is built (seen
on the suite's and Mandelbrot's Flatpak jobs, 2026-11-13). Copying the
installation first lets the update work on every runner.
"""

import glob
import os
import unittest

import yaml

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
STEP = "Make the image's Flatpak installation writable"


class FlatpakJobs(unittest.TestCase):
    def test_the_installation_is_made_writable_before_building(self):
        missing = []
        for path in sorted(glob.glob(os.path.join(ROOT, ".github", "workflows", "*.yml"))):
            with open(path, encoding="utf-8") as f:
                wf = yaml.safe_load(f)
            for job_id, job in (wf.get("jobs") or {}).items():
                image = str((job.get("container") or {}).get("image", ""))
                if "flathub-infra/flatpak-github-actions" not in image:
                    continue
                steps = job.get("steps") or []
                builds = [i for i, s in enumerate(steps)
                          if str(s.get("uses", "")).startswith("flatpak/flatpak-github-actions/flatpak-builder")]
                if not builds:
                    continue
                copies = [i for i, s in enumerate(steps) if s.get("name") == STEP]
                if not copies or copies[0] > builds[0]:
                    missing.append(f"{os.path.relpath(path, ROOT)}: {job_id}")
        self.assertEqual(missing, [], "flatpak-builder runs on the image's own installation: " + ", ".join(missing))


if __name__ == "__main__":
    unittest.main()
