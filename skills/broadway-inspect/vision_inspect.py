#!/usr/bin/env python3
"""
Broadway Vision Inspector — uses local Lemonade AI to read text from Broadway screenshots.
GTK4 Broadway renders text as rasterized images, not DOM text nodes.
This script captures screenshots and uses a vision model to extract text content.

Usage: python3 skills/broadway-inspect/vision_inspect.py [letters|tables|decks]

Requires: Letters running via Broadway at http://localhost:8085
          Lemonade server at https://lemonade.manatee-basking.ts.net/v1
"""

import base64
import json
import sys
import time
import urllib.request
from playwright.sync_api import sync_playwright

BROADWAY_URL = "http://localhost:8085"
LEMONADE_URL = "https://lemonade.manatee-basking.ts.net/v1/chat/completions"
MODEL = "Gemma-4-12B-it-GGUF"

def vision_query(image_path: str, prompt: str) -> str:
    """Send an image + prompt to the Lemonade vision model."""
    with open(image_path, "rb") as f:
        image_b64 = base64.b64encode(f.read()).decode()

    body = {
        "model": MODEL,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": prompt},
                {"type": "image_url", "image_url": {"url": f"data:image/png;base64,{image_b64}"}}
            ]
        }],
        "max_tokens": 300,
        "temperature": 0.1
    }

    req = urllib.request.Request(
        LEMONADE_URL,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"}
    )

    with urllib.request.urlopen(req, timeout=60) as resp:
        result = json.loads(resp.read())
        return result["choices"][0]["message"]["content"]

def run():
    app = sys.argv[1] if len(sys.argv) > 1 else "letters"
    print(f"=== Broadway Vision Inspector: {app} ===\n")

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True, args=['--enable-webgl','--use-gl=swiftshader','--no-sandbox'])
        page = browser.new_page(viewport={"width": 1024, "height": 768})
        page.goto(BROADWAY_URL, timeout=15000)
        page.wait_for_timeout(3000)

        # Create new document + type text
        page.keyboard.press("Control+n")
        page.wait_for_timeout(2000)
        page.keyboard.type("Hello from Letters!", delay=20)
        page.keyboard.press("Enter")
        page.keyboard.type("This is bold text.", delay=20)
        page.keyboard.press("Control+a")
        page.keyboard.press("Control+b")
        page.keyboard.press("ArrowRight")
        page.wait_for_timeout(1000)

        # Screenshot
        path = "/tmp/broadway-vision.png"
        page.screenshot(path=path)
        print(f"Screenshot: {path}")

        # Vision model analysis
        print("Sending to vision model…")
        start = time.time()

        prompt = """Describe this GTK4 application window screenshot concisely:
1. What text do you see in the editor area?
2. Is there any formatting visible (bold, italic)?
3. What toolbar buttons/labels are visible?
4. Is there a ruler, page background, or status bar?
5. What is the overall layout?"""

        try:
            result = vision_query(path, prompt)
            elapsed = time.time() - start
            print(f"\n=== Vision Model Response ({elapsed:.1f}s) ===\n{result}")
        except Exception as e:
            print(f"Vision model error: {e}")
            print("(Is the Lemonade server running? https://lemonade.manatee-basking.ts.net)")

        browser.close()

if __name__ == "__main__":
    run()
