#!/usr/bin/env python3
"""Ratchet file and Rust function sizes; optionally report duplicated bodies."""
from __future__ import annotations

import hashlib
import re
import subprocess
import sys
import tomllib
from pathlib import Path

FILE_CAP, FN_CAP, DUP_MIN = 500, 80, 5
EXTS = ('*.rs', '*.py', '*.sh', '*.yml')
FN = re.compile(r'^[ \t]*(?:pub(?:\([a-z:]+\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+(\w+)', re.M)
TESTS = re.compile(r'/tests/|(^|/)tests\.rs$|_tests\.rs$|/examples/|^tools/')
DEFAULT_EXEMPT = ['crates/animated-icons/src/source.rs', 'crates/epub/src/generated.rs']
ROOT = Path(subprocess.run(['git', 'rev-parse', '--show-toplevel'], capture_output=True, text=True, check=True).stdout.strip())
BASELINE = ROOT / 'tools/shape_baseline.toml'


def functions(text: str):
    for found in FN.finditer(text):
        brace, semi = text.find('{', found.end()), text.find(';', found.end())
        if brace < 0 or 0 <= semi < brace:
            continue
        depth, end = 0, brace
        for end in range(brace, len(text)):
            depth += (text[end] == '{') - (text[end] == '}')
            if depth == 0:
                break
        body = re.sub(r'//[^\n]*', '', text[brace:end + 1])
        yield found.group(1), text.count('\n', found.start(), end) + 1, ' '.join(body.split())


def measure(exempt):
    files, fns, bodies = {}, {}, {}
    paths = subprocess.run(['git', '-C', str(ROOT), 'ls-files', '--', *EXTS], capture_output=True, text=True, check=True).stdout.splitlines()
    for path in paths:
        if path in exempt or not (ROOT / path).is_file():
            continue
        text = (ROOT / path).read_text(encoding='utf-8', errors='replace')
        files[path] = len(text.splitlines())
        if not path.endswith('.rs') or TESTS.search(path):
            continue
        for name, lines, body in functions(text):
            key = f'{path}::{name}'
            fns[key] = max(lines, fns.get(key, 0))
            if lines >= DUP_MIN:
                digest = hashlib.sha256(body.encode()).hexdigest()
                bodies.setdefault(digest, set()).add(path)
    return files, fns, bodies


def write(ceiling, fn_ceiling, exempt):
    out = ['# Written by tools/shape_check.py. Ceilings only go down.', '', '[exempt]',
           'paths = [' + ', '.join(f'"{p}"' for p in exempt) + ']', '', '[ceiling]']
    out += [f'"{path}" = {n}' for path, n in sorted(ceiling.items())]
    out += ['', '[functions]']
    out += [f'"{key}" = {n}' for key, n in sorted(fn_ceiling.items())]
    BASELINE.write_text('\n'.join(out) + '\n', encoding='utf-8')


def main():
    args = set(sys.argv[1:])
    if args - {'--init', '--tighten', '--dups'} or len(args & {'--init', '--tighten'}) > 1:
        print('usage: shape_check.py [--init | --tighten] [--dups]')
        return 2
    data = tomllib.loads(BASELINE.read_text(encoding='utf-8')) if BASELINE.exists() else {}
    exempt = data.get('exempt', {}).get('paths', DEFAULT_EXEMPT)
    ceiling, fn_ceiling = data.get('ceiling', {}), data.get('functions', {})
    files, fns, bodies = measure(exempt)
    if '--init' in args:
        if BASELINE.exists():
            print('shape: baseline exists; use --tighten')
            return 1
        big = {p: n for p, n in files.items() if n > FILE_CAP}
        long = {k: n for k, n in fns.items() if n > FN_CAP}
        write(big, long, exempt)
        print(f'shape: baseline written, {len(big)} files and {len(long)} functions')
        return 0
    if not BASELINE.exists():
        print('shape: baseline missing; run --init')
        return 1
    if '--tighten' in args:
        big = {p: min(files[p], n) for p, n in ceiling.items() if files.get(p, 0) > FILE_CAP}
        long = {k: min(fns[k], n) for k, n in fn_ceiling.items() if fns.get(k, 0) > FN_CAP}
        write(big, long, exempt)
        print(f'shape: tightened to {len(big)} files and {len(long)} functions')
        return 0
    errors = []
    for path, n in sorted(files.items()):
        limit = ceiling.get(path, FILE_CAP)
        if n > limit:
            errors.append(f'{path}: {n} lines, over limit {limit}')
    for key, n in sorted(fns.items()):
        limit = fn_ceiling.get(key, FN_CAP)
        if n > limit:
            errors.append(f'{key}: {n} lines, over limit {limit}')
    if '--dups' in args:
        for paths in bodies.values():
            if len(paths) > 1:
                errors.append('the same function body lives in: ' + ', '.join(sorted(paths)))
    for line in errors:
        print(line)
    print(f'shape: {len(errors)} breach(es)' if errors else f'shape: {len(files)} files ok')
    return int(bool(errors))


if __name__ == '__main__':
    sys.exit(main())
