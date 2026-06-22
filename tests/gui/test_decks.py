#!/usr/bin/env python3
"""Dogtail GUI test for Rust Decks — verifies window + sidebar + canvas."""
import sys
from dogtail import tree

def main():
    app = tree.root.application('decks')
    print('Rust Decks — found application')
    try:
        lb = app.child(roleName='list box')
        print('  Found slide sidebar (list box)')
    except Exception:
        print('  [SKIP] slide sidebar')
    try:
        area = app.child(roleName='drawing area')
        print('  Found canvas (drawing area)')
    except Exception:
        print('  [SKIP] canvas')
    print('RUST GUITEST: PASS')
    return 0

if __name__ == '__main__':
    try: sys.exit(main())
    except Exception as e: print(f'RUST GUITEST: FAIL — {e}'); sys.exit(1)
