#!/usr/bin/env python3
"""Dogtail GUI test for Rust Tables — verifies app launches and renders."""
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
    print("Searching for Tables application...")
    app = None
    for attempt in range(15):
        for name in ['org.tunaos.tables', 'tables', 'Tables']:
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
        raise RuntimeError("Could not find Tables application")
        
    print(f"Found Tables: {app.name}")
    
    print("Accessibility tree structure:")
    dump_tree(app)
    
    if app.child_count == 0:
        raise ValueError("Application accessibility tree is empty")
        
    # Verify formula entry / text inputs
    entries = wait_for_child(app, lambda x: x.roleName in ['text', 'entry'])
    print(f"Found {len(entries)} text/entry inputs")
    
    # Verify buttons
    buttons = wait_for_child(app, lambda x: x.roleName in ['push button', 'toggle button'])
    button_names = [b.name for b in buttons]
    print(f"Found buttons: {button_names}")
    
    # Check for 'Open' and 'Save' buttons
    assert 'Open' in button_names, "Missing 'Open' button"
    assert 'Save' in button_names, "Missing 'Save' button"
    
    # Check for format buttons
    for btn in ['B', 'I', 'U']:
        assert any(b.startswith(btn) for b in button_names), f"Missing formatting button: {btn}"
        
    print("RUST GUITEST: PASS")
    return 0

if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception as e:
        print(f"RUST GUITEST: FAIL — {e}")
        sys.exit(1)


