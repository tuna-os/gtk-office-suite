"""VLM/vision-based assertions, split out of BaseGUITestCase.

Only test files that actually judge screenshots with a vision-language
model (test_letters.py, test_tables.py, test_decks.py,
test_design_gnome_hig.py, test_gnome_hig_compliance.py) need
VisionGUITestCase. The deterministic AT-SPI suite (test_smoke.py) and the
harness's own self-tests (test_harness_artifacts.py) inherit
BaseGUITestCase directly and never see VLM backend selection, API keys, or
response-parsing code — that isolation is the reason test_smoke.py exists
as a separate file in the first place (see its module docstring).
"""

import base64
import json
import os
import re
from io import BytesIO

import requests
from PIL import Image

from .base import BaseGUITestCase


class VisionGUITestCase(BaseGUITestCase):
    """BaseGUITestCase plus VLM-backed screenshot assertions."""

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
