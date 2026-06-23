#!/usr/bin/env python3
"""
Broadway DOM Analyzer — extracts meaningful UI information from GTK4 Broadway render output.
Broadway renders GTK widgets as real DOM nodes with CSS transforms, colors, and images.

Usage: python3 skills/broadway-inspect/broadway_inspect.py [letters|tables|decks]
"""

import sys
from playwright.sync_api import sync_playwright

BROADWAY_URL = "http://localhost:8085"

def run():
    app = sys.argv[1] if len(sys.argv) > 1 else "letters"
    print(f"=== Broadway DOM Analyzer: {app} ===")

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True, args=['--enable-webgl','--use-gl=swiftshader','--no-sandbox'])
        page = browser.new_page(viewport={"width": 1024, "height": 768})
        page.goto(BROADWAY_URL, timeout=15000)
        page.wait_for_timeout(5000)

        dom_size = len(page.content())
        print(f"DOM size: {dom_size} chars")

        # Check if broadway is rendering (needs >500 chars to have actual content)
        if dom_size < 1000:
            print("NO RENDER: Broadway page has no widget DOM (only JS shell)")
            browser.close()
            return

        # Extract meaningful UI elements
        findings = page.evaluate("""() => {
            const results = [];
            const divs = document.querySelectorAll('div');
            
            // Look for specific widget patterns:
            // - Large containers (likely PageContainer or Window)
            // - Images (icons, illustrations)
            // - Clickable areas with text labels
            divs.forEach(d => {
                const rect = d.getBoundingClientRect();
                const w = Math.round(rect.width);
                const h = Math.round(rect.height);
                const bg = d.style.backgroundColor || '';
                const shadow = d.style.boxShadow || '';
                
                // Large containers
                if (w > 400 && h > 400) {
                    results.push({
                        type: 'container',
                        size: w + 'x' + h,
                        bg: bg,
                        shadow: shadow ? 'yes' : 'no'
                    });
                }
            });
            
            // Find images
            const imgs = document.querySelectorAll('img');
            imgs.forEach(img => {
                results.push({
                    type: 'image',
                    size: img.width + 'x' + img.height
                });
            });
            
            // Find text content
            const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
            const texts = [];
            let node;
            while (node = walker.nextNode()) {
                const t = node.textContent.trim();
                if (t && t.length > 1 && t.length < 100) {
                    texts.push(t);
                }
            }
            if (texts.length > 0) {
                results.push({type: 'text_count', count: texts.length});
            }
            
            return {widgets: results, texts: texts.slice(0, 30)};
        }""")

        print(f"\n=== Widget Analysis ===")
        for w in findings.get('widgets', []):
            t = w.get('type', '?')
            if t == 'container':
                print(f"  [{w['size']}] Page/Window container (bg: {w.get('bg','?')}, shadow: {w.get('shadow','?')})")
            elif t == 'image':
                print(f"  [IMG] {w['size']}")
            elif t == 'text_count':
                print(f"  [TEXT] {w['count']} text nodes found")

        # Show text content
        texts = findings.get('texts', [])
        if texts:
            print(f"\n=== UI Text Content (first 20) ===")
            for t in texts[:20]:
                print(f"  '{t}'")

        # Screenshot
        page.screenshot(path='/tmp/broadway-letters.png')
        print("\nScreenshot: /tmp/broadway-letters.png")

        # Summary
        print(f"\n=== Summary ===")
        if dom_size > 5000:
            print("✅ Broadway rendering ACTIVE — widgets present in DOM")
        else:
            print("❌ No widget rendering detected")
        
        has_containers = any(w.get('type') == 'container' for w in findings.get('widgets', []))
        has_images = any(w.get('type') == 'image' for w in findings.get('widgets', []))
        has_texts = len(texts) > 0
        
        if has_containers:
            print("✅ Page container widget detected")
        if has_images:
            print("✅ Icons/illustrations detected")
        if has_texts:
            print(f"✅ {len(texts)} text nodes found (toolbar labels, status text)")

        browser.close()

if __name__ == "__main__":
    run()
