#!/usr/bin/env python3
"""Dogtail GUI test for Rust Letters — verifies app launches and renders."""
import sys
import time
from dogtail.config import config
config.checkForA11y = False
from dogtail import tree
from dogtail.rawinput import typeText, pressKey

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
    print("Searching for Letters application...")
    app = None
    for attempt in range(15):
        for name in ['org.tunaos.letters', 'letters', 'Letters']:
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
        raise RuntimeError("Could not find Letters application")
        
    print(f"Found Letters: {app.name}")
    
    print("Accessibility tree structure:")
    dump_tree(app)
    
    if app.child_count == 0:
        raise ValueError("Application accessibility tree is empty")
        
    # Check buttons
    buttons = wait_for_child(app, lambda x: x.roleName in ['push button', 'toggle button'])
    button_names = [b.name for b in buttons]
    print(f"Found buttons: {button_names}")
    
    assert 'Open' in button_names, "Missing 'Open' button"
    assert 'Save' in button_names, "Missing 'Save' button"
    assert 'Insert Table' in button_names or 'Table' in button_names, "Missing 'Table' button"
    assert 'Find and Replace' in button_names or 'Find' in button_names, "Missing 'Find' button"
    
    # Check the editor (TextView). In GTK, it has role 'text' or 'document'
    editors = wait_for_child(app, lambda x: x.roleName in ['text', 'document', 'document web', 'text view'])
    print(f"Found editors: {[(e.name, e.roleName) for e in editors]}")
    editor = editors[0]
    
    # Let's type some text
    editor.click()
    time.sleep(0.5)
    
    # Check word counter
    labels = wait_for_child(app, lambda x: x.roleName in ['label', 'text', 'static'])
    status_labels = [l for l in labels if 'words' in str(l.name)]
    print(f"Found word count labels: {[l.name for l in status_labels]}")
    
    # Type "Hello world "
    editor.typeText("Hello world ")
    
    # Wait for word counter to update (polled)
    success = False
    for _ in range(10):
        labels = app.findChildren(lambda x: x.roleName in ['label', 'text', 'static'])
        status_labels = [l for l in labels if 'words' in str(l.name)]
        if any('2 words' in str(l.name) for l in status_labels):
            success = True
            break
        time.sleep(0.5)
        
    print(f"Word count labels after typing: {[l.name for l in status_labels]}")
    assert success, "Word count label did not update to '2 words'"
    
    # Enter a newline and type "# Header" to trigger markdown macro
    pressKey("Return")
    editor.typeText("# My Header ")
    time.sleep(0.5)
    
    # Click Find button to toggle Find and Replace panel
    find_btn = None
    for name in ['Find and Replace', 'Find']:
        try:
            find_btn = app.child(name, roleName='push button')
            if find_btn:
                break
        except Exception:
            pass
    assert find_btn is not None, "Missing 'Find' button"
    find_btn.click()
    
    # Wait for Find/Replace panel buttons to be visible
    success = False
    for _ in range(10):
        buttons = app.findChildren(lambda x: x.roleName == 'push button')
        button_names = [b.name for b in buttons]
        if 'Next' in button_names:
            success = True
            break
        time.sleep(0.5)
        
    assert success, "Missing 'Next' button after clicking Find"
    
    print("RUST GUITEST: PASS")
    return 0

if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception as e:
        print(f"RUST GUITEST: FAIL — {e}")
        sys.exit(1)


