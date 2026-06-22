#!/usr/bin/env python3
"""Dogtail GUI test for Rust Decks — verifies app launches and renders."""
import sys
import time
from dogtail.config import config
config.checkForA11y = False
from dogtail import tree

def wait_for_child(parent, check_func, timeout=10):
    start = time.time()
    while time.time() - start < timeout:
        children = parent.findChildren(check_func)
        if children:
            return children
        time.sleep(0.5)
    raise RuntimeError("Timed out waiting for child matching condition")

def dump_tree(node, depth=0):
    indent = "  " * depth
    print(f"{indent}- name: {node.name}, role: {node.roleName}")
    for child in node.children:
        try:
            dump_tree(child, depth + 1)
        except Exception:
            pass

def main():
    print("Searching for Decks application...")
    app = None
    for attempt in range(15):
        for name in ['org.tunaos.decks', 'decks', 'Decks']:
            try:
                app = tree.root.application(name)
                if app:
                    break
            except Exception:
                pass
        if app:
            break
        time.sleep(1)
        
    if not app:
        print("Active applications:")
        for c in tree.root.children:
            print(f"  - {c.name} ({c.roleName})")
        raise RuntimeError("Could not find Decks application")
        
    print(f"Found Decks: {app.name}")
    
    print("Accessibility tree structure:")
    dump_tree(app)
    
    if app.child_count == 0:
        raise ValueError("Application accessibility tree is empty")
        
    # Check buttons
    buttons = wait_for_child(app, lambda x: x.roleName in ['push button', 'toggle button'])
    button_names = [b.name for b in buttons]
    print(f"Found buttons: {button_names}")
    
    assert 'Insert Text Box' in button_names or 'Text' in button_names, "Missing 'Text' button"
    assert 'Insert Rectangle' in button_names or 'Rect' in button_names, "Missing 'Rect' button"
    assert 'Add Slide' in button_names, "Missing 'Add Slide' button"
    
    # Check slide sidebar list box
    listboxes = wait_for_child(app, lambda x: x.roleName == 'list')
    print(f"Found list boxes: {[(l.name, l.roleName) for l in listboxes]}")
    sidebar = listboxes[0]
    
    initial_slides = sidebar.child_count
    print(f"Initial slide count: {initial_slides}")
    assert initial_slides > 0, "Slide sidebar is empty"
    
    # Click "Add Slide" button
    add_slide_btn = None
    for name in ['Add Slide']:
        try:
            add_slide_btn = app.child(name, roleName='push button')
            if add_slide_btn:
                break
        except Exception:
            pass
    assert add_slide_btn is not None, "Missing 'Add Slide' button"
    add_slide_btn.click()
    
    # Verify that the slide count increased, using polling
    success = False
    for _ in range(10):
        if sidebar.child_count == initial_slides + 1:
            success = True
            break
        time.sleep(0.5)
        
    new_slides = sidebar.child_count
    print(f"Slide count after Add Slide: {new_slides}")
    assert success, f"Slide count did not increase (expected {initial_slides + 1}, got {new_slides})"
    
    print("RUST GUITEST: PASS")
    return 0

if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception as e:
        print(f"RUST GUITEST: FAIL — {e}")
        sys.exit(1)


