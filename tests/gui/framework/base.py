import os
import sys
import time
import json
import base64
import re
import subprocess
import unittest
from io import BytesIO

import mss
import requests
from PIL import Image
from dogtail import tree, rawinput

_dogtail_click = rawinput.click
_dogtail_key_combo = rawinput.keyCombo
_dogtail_type_text = rawinput.typeText

class BaseGUITestCase(unittest.TestCase):
    app_name = None  # to be overridden by subclasses

    @classmethod
    def setUpClass(cls):
        if not cls.app_name:
            raise unittest.SkipTest("app_name not set")

    def setUp(self):
        # Resolve directories
        self.framework_dir = os.path.dirname(os.path.abspath(__file__))
        self.gui_dir = os.path.dirname(self.framework_dir)
        self.workspace_dir = os.path.dirname(os.path.dirname(self.gui_dir))

        # Path to compiled binary
        self.bin_path = os.path.join(self.workspace_dir, "target", "debug", self.app_name)
        if not os.path.exists(self.bin_path):
            raise RuntimeError(f"Binary not found at {self.bin_path}. Run 'cargo build' first.")

        # Clear any leftover processes
        subprocess.run(["pkill", "-x", self.app_name], stderr=subprocess.DEVNULL)
        time.sleep(0.2)

        # Launch app under GDK_BACKEND=x11
        env = os.environ.copy()
        env["GDK_BACKEND"] = "x11"
        env.update(getattr(self, "launch_env", {}))
        self.process = subprocess.Popen(
            [self.bin_path] + list(getattr(self, "launch_args", [])),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True
        )

        # Wait for application node in AT-SPI tree
        self.app = self.wait_for_app(self.app_name)
        self._activate_window()
        self.last_screenshot = None

    def tearDown(self):
        rawinput.click = _dogtail_click
        rawinput.keyCombo = _dogtail_key_combo
        rawinput.typeText = _dogtail_type_text
        if hasattr(self, "process") and self.process:
            self.process.terminate()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()

    def wait_for_process_exit(self, timeout: float = 5.0) -> "int | None":
        """Poll self.process until it exits or the timeout elapses. A fixed
        sleep before checking poll() is a common source of flaky failures
        under load (Xvfb/AT-SPI daemon contention slows real exits down);
        this waits only as long as actually needed."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            code = self.process.poll()
            if code is not None:
                return code
            time.sleep(0.1)
        return self.process.poll()

    def _activate_window(self):
        """dogtail.rawinput's keyboard/mouse synthesis goes through AT-SPI's
        DeviceEventController (Registry.generateKeyboardEvent/
        generateMouseEvent), which is a D-Bus call asking the AT-SPI
        registry daemon to synthesize XTEST events on the caller's behalf.
        In this harness that call succeeds without error but the events
        never arrive at the app — confirmed by direct probing: xdotool's
        own XTest calls (which bypass the AT-SPI registry entirely) work
        correctly against the exact same window in the exact same
        session. So this replaces the 3 rawinput entry points test_smoke
        actually uses (click/keyCombo/typeText) with xdotool-backed
        equivalents scoped to this test's window, rather than trying to
        fix the AT-SPI daemon's XTest bridge itself.

        Re-activating the window before every call (not just once here)
        matters too: a test that clicks "New Document" or opens a dialog
        after setUp can end up with a widget that never got real X focus,
        since GTK's internal focus-follows-widget-creation doesn't
        re-assert top-level X focus on its own."""
        win_id = None
        try:
            result = subprocess.run(
                ["xdotool", "search", "--pid", str(self.process.pid)],
                capture_output=True, text=True, timeout=5,
            )
            win_ids = [w for w in result.stdout.split() if w]
            if win_ids:
                win_id = win_ids[-1]
                subprocess.run(["xdotool", "windowactivate", "--sync", win_id],
                                capture_output=True, timeout=5)
        except (FileNotFoundError, subprocess.TimeoutExpired) as e:
            print(f"Warning: window activation failed ({e}); rawinput tests may not receive input")
            return
        if win_id is None:
            return

        def reactivate():
            try:
                subprocess.run(["xdotool", "windowactivate", "--sync", win_id],
                                capture_output=True, timeout=2)
            except (FileNotFoundError, subprocess.TimeoutExpired):
                pass

        def click(x, y, button=1, check=True):
            reactivate()
            subprocess.run(
                ["xdotool", "mousemove", "--sync", str(int(x)), str(int(y)),
                 "click", str(button)],
                capture_output=True, timeout=5,
            )

        def key_combo(combo_string):
            # Dogtail's own syntax: '<Control>k' -> tokens ['Control', 'k'].
            # Reuse its alias table (control -> Control_L etc.) so the
            # tokens are valid X keysym names, then join for xdotool.
            tokens = []
            for s in combo_string.split('<'):
                if s:
                    for tok in s.split('>'):
                        if tok:
                            tokens.append(rawinput.keyNameAliases.get(tok.lower(), tok))
            reactivate()
            subprocess.run(
                ["xdotool", "key", "--window", win_id, "+".join(tokens)],
                capture_output=True, timeout=5,
            )

        def type_text(text):
            reactivate()
            subprocess.run(
                ["xdotool", "type", "--window", win_id, "--", text],
                capture_output=True, timeout=10,
            )

        rawinput.click = click
        rawinput.keyCombo = key_combo
        rawinput.typeText = type_text

    def wait_for_app(self, name: str, timeout: float = 15.0) -> "tree.Node":
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                app = tree.root.application(name)
                if app:
                    return app
            except Exception:
                pass
            time.sleep(0.5)
        raise RuntimeError(f"Application '{name}' did not appear in AT-SPI registry within {timeout}s")

    def get_window_geometry(self) -> tuple[int, int, int, int]:
        """Locates the main frame window of the app and returns (x, y, w, h)."""
        try:
            frame = self.app.child(roleName='frame')
            if frame:
                return frame.position[0], frame.position[1], frame.size[0], frame.size[1]
        except Exception as e:
            print(f"Warning: Failed to get frame geometry via Dogtail: {e}")
        return 0, 0, 1920, 1080

    # ── Vision verification ──────────────────────────────────────────────
    VLM_BACKEND = os.environ.get("VLM_BACKEND", "gemini")
    LEMONADE_URL = "https://lemonade.manatee-basking.ts.net/v1/chat/completions"
    LEMONADE_MODEL = os.environ.get("VLM_LEMONADE_MODEL", "Gemma-4-31B-it-GGUF")
    GEMINI_API_KEY = os.environ.get("GEMINI_API_KEY", "")
    GEMINI_API_KEY_2 = os.environ.get("GEMINI_API_KEY_2", "")
    GEMINI_MODEL = os.environ.get("VLM_GEMINI_MODEL", "gemini-2.5-flash")
    _gemini_key_tried = False

    def _vlm_request(self, image_b64: str, prompt: str, model: str = None) -> str:
        """Send an image + prompt to the configured VLM backend and return the text response."""
        backend = self.VLM_BACKEND

        if backend == "lemonade":
            model = model or self.LEMONADE_MODEL
            resp = requests.post(
                self.LEMONADE_URL,
                json={
                    "model": model,
                    "messages": [{
                        "role": "user",
                        "content": [
                            {"type": "text", "text": prompt},
                            {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{image_b64}"}},
                        ]
                    }],
                    "max_tokens": 512,
                },
                timeout=120,
            )
            resp.raise_for_status()
            data = resp.json()
            msg = data["choices"][0]["message"]
            return msg.get("reasoning_content") or msg.get("content") or ""

        elif backend == "gemini":
            if not self.GEMINI_API_KEY:
                self.skipTest("GEMINI_API_KEY not set")
            model = model or self.GEMINI_MODEL
            keys = [self.GEMINI_API_KEY]
            if self.GEMINI_API_KEY_2:
                keys.append(self.GEMINI_API_KEY_2)
            last_error = None
            for key in keys:
                url = f"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={key}"
                resp = requests.post(url, json={
                    "contents": [{"parts": [
                        {"inline_data": {"mime_type": "image/jpeg", "data": image_b64}},
                        {"text": prompt},
                    ]}],
                }, timeout=30)
                if resp.status_code == 429:
                    last_error = "Rate limited (429), trying fallback key"
                    continue
                if resp.status_code == 503:
                    last_error = "Model overloaded (503), trying fallback"
                    continue
                resp.raise_for_status()
                data = resp.json()
                candidates = data.get("candidates", [])
                if candidates:
                    parts = candidates[0].get("content", {}).get("parts", [])
                    return "".join(p.get("text", "") for p in parts)
                return ""
            raise RuntimeError(f"Gemini API failed: {last_error}")

        else:
            raise ValueError(f"Unknown VLM_BACKEND: {backend}. Use 'lemonade' or 'gemini'.")

    def assertVision(
        self,
        checks: list,
        screenshot_path: str = None,
        model: str = None,
    ):
        """
        Assert visual UI state against a list of checks using a VLM.

        Each check is either:
          - a string (auto-named assertion)
          - a dict {"name": "...", "prompt": "..."}
        """
        # Normalise checks to list of dicts
        normalised = []
        for i, c in enumerate(checks):
            if isinstance(c, str):
                normalised.append({"name": f"check-{i}", "prompt": c})
            else:
                normalised.append(c)

        # Capture screenshot if not provided
        if screenshot_path is None:
            self.take_screenshot("vlm")
            screenshot_path = os.path.join(self.gui_dir, f"{self.app_name}_screenshot_vlm.png")

        if not os.path.exists(screenshot_path):
            self.fail(f"Screenshot not found: {screenshot_path}")

        # Resize and encode
        img = Image.open(screenshot_path)
        img.thumbnail((800, 600), Image.LANCZOS)
        buf = BytesIO()
        img.save(buf, format="JPEG", quality=70)
        image_b64 = base64.b64encode(buf.getvalue()).decode()

        # Build structured prompt
        checks_json = json.dumps([
            {"id": c["name"], "assertion": c["prompt"]}
            for c in normalised
        ], indent=2)

        prompt = (
            "You are a GUI testing assistant. Verify each assertion about the screenshot.\n\n"
            f"Assertions:\n{checks_json}\n\n"
            "For each assertion, say \"Result: Pass.\" or \"Result: Fail.\" with brief evidence."
        )

        response = self._vlm_request(image_b64, prompt, model=model)

        # ── Parse VLM response ──────────────────────────────────────────
        # The reasoning model outputs structured text like:
        #   **Assertion check-0: ...**
        #   ... evidence ...
        #   Result: Pass.
        # We parse this directly.

        results = []
        for c in normalised:
            cid = c["name"]
            # Search for this check in response
            idx = response.lower().find(cid.lower())
            if idx < 0:
                # Try the first 20 chars of the assertion text
                short = c["prompt"][:20].lower()
                idx = response.lower().find(short)
            if idx < 0:
                idx = 0

            start = max(0, idx - 30)
            para = response[start:start+600]

            passed = None
            if re.search(r'Result\s*[:.]\s*Pass', para, re.IGNORECASE):
                passed = True
            elif re.search(r'Result\s*[:.]\s*Fail', para, re.IGNORECASE):
                passed = False
            elif re.search(r'Status\s*[:.]\s*Pass', para, re.IGNORECASE):
                passed = True
            elif re.search(r'Status\s*[:.]\s*Fail', para, re.IGNORECASE):
                passed = False
            elif re.search(r'\bPASS\b', para):
                passed = True
            elif re.search(r'\bFAIL\b', para):
                passed = False

            # Extract evidence
            ev = para.strip()
            ev = re.sub(r'^[\d.\s*\-`#_~]+', '', ev).strip()
            evidence = ev[:250]

            if passed is not None:
                results.append({"id": cid, "pass": passed, "evidence": evidence})
            else:
                print(f"  ? {cid}: ambiguous VLM output, defaulting to FAIL")
                results.append({"id": cid, "pass": False, "evidence": "Could not determine pass/fail from VLM"})

        if not results:
            print(f"? VLM response could not be parsed. Raw:\n{response[:500]}")
            self.fail(f"VLM assertion failed: could not parse response for {[c['name'] for c in normalised]}")
            return

        # ── Log results and assert ──────────────────────────────────────
        all_pass = True
        for r in results:
            cid = r.get("id", "?")
            passed = r.get("pass", False)
            evidence = r.get("evidence", "")
            status = "PASS" if passed else "FAIL"
            icon = "+" if passed else "x"
            print(f"  [{icon}] {cid}: {status} — {evidence}")
            if not passed:
                all_pass = False

        reported_ids = {r.get("id") for r in results}
        for c in normalised:
            if c["name"] not in reported_ids:
                print(f"  [?] {c['name']}: not evaluated by VLM")
                all_pass = False

        self.assertTrue(all_pass, f"{len([r for r in results if not r.get('pass', False)])} visual assertion(s) failed")

    # ── Screenshot helpers ─────────────────────────────────────────────────

    def take_screenshot(self, suffix: str, crop: bool = True) -> str | None:
        """Takes a screenshot, stores path in self.last_screenshot, returns it."""
        output_filename = f"{self.app_name}_screenshot"
        if suffix:
            output_filename += f"_{suffix}"
        output_filename += ".png"

        output_path = os.path.join(self.gui_dir, output_filename)

        display = os.environ.get("DISPLAY")
        if not display:
            print("Warning: DISPLAY not set, skipping screenshot.")
            return None

        try:
            with mss.mss() as sct:
                sct.shot(output=output_path)

            if crop:
                x, y, w, h = self.get_window_geometry()
                if w > 0 and h > 0:
                    img = Image.open(output_path)
                    cropped_img = img.crop((x, y, x + w, y + h))
                    cropped_img.save(output_path)
                    print(f"Saved window-cropped screenshot to {output_path}")
                else:
                    print(f"Saved full-screen screenshot to {output_path} (invalid geometry: {x},{y},{w},{h})")
            else:
                print(f"Saved full-screen screenshot to {output_path}")
            self.last_screenshot = output_path
            return output_path
        except Exception as e:
            print(f"Failed to capture screenshot: {e}")
            return None
