#!/usr/bin/env python3
"""Check the budget for non-doc Rust line comments added in a diff."""
import subprocess
import sys

RATIO, FLOOR = 25, 2


def main():
    base = sys.argv[1] if len(sys.argv) > 1 else 'origin/main'
    diff = subprocess.run(['git', 'diff', '-U0', base, '--', '*.rs'], capture_output=True, text=True, check=True).stdout
    code = comments = 0
    for line in diff.splitlines():
        if not line.startswith('+') or line.startswith('+++'):
            continue
        text = line[1:].strip()
        if not text or text.startswith(('///', '//!')):
            continue
        if text.startswith('//'):
            comments += 1
        else:
            code += 1
    allowed = max(FLOOR, code // RATIO)
    if comments > allowed:
        print(f'comment budget: {comments} comments for {code} code lines; limit {allowed}')
        return 1
    print(f'comment budget ok: {comments} of {allowed}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
