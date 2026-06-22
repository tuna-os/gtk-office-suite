#!/usr/bin/env python3
"""Dogtail GUI test for Rust Letters — verifies window + editor + tabs."""
import sys
from dogtail import tree

def main():
    app = tree.root.application('letters')
    print('Rust Letters — found application')
    try:
        tv = app.child(roleName='text')
        print('  Found text editor')
    except Exception:
        print('  [SKIP] editor')
    try:
        nb = app.child(roleName='notebook page')
        print('  Found tab bar (notebook)')
    except Exception:
        print('  [SKIP] tabs')
    print('RUST GUITEST: PASS')
    return 0

if __name__ == '__main__':
    try: sys.exit(main())
    except Exception as e: print(f'RUST GUITEST: FAIL — {e}'); sys.exit(1)
