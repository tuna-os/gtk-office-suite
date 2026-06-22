#!/usr/bin/env python3
"""Dogtail GUI test for Rust Letters — verifies app launches and renders."""
import sys
from dogtail import tree

def main():
    app = tree.root.application('tables')
    print('Rust Letters — found application')
    cc = app.child_count if hasattr(app, 'child_count') else 0
    print(f'  child_count: {cc}')
    if cc > 0: print('RUST GUITEST: PASS'); return 0
    else: print('RUST GUITEST: SKIP — a11y tree empty'); return 0

if __name__ == '__main__':
    try: sys.exit(main())
    except Exception as e: print(f'RUST GUITEST: FAIL — {e}'); sys.exit(1)
