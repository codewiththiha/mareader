#!/usr/bin/env python3
"""Report code that cannot change what the reader sees.

    python3 tools/dead_check.py <file or directory> [--json] [--limit N]

Three reads. CSS: a rule that declares one property twice, where the earlier
declaration can never win, and a custom property nothing consumes. Rust: a `pub`
item no other file names, and a settings field that is written but never read.

Every finding is a lead to confirm by reading, not a verdict: a `pub` item can
be a deliberate crate boundary, and a field can be read through a derive.
Vendored trees are skipped: they are not ours to prune.
`dead-check: allow` on a line suppresses its finding.
"""

import argparse
import json
import re
import sys
from pathlib import Path

ALLOW = "dead-check: allow"
CODE = {".rs", ".css", ".ts", ".mjs", ".js"}
RULE = re.compile(r"([^{}]+)\{([^{}]*)\}", re.S)
DECL = re.compile(r"([A-Za-z-]+)\s*:\s*[^;{}]+")
PROP = re.compile(r"[{;]\s*(--[a-z0-9-]+)\s*:")
PUB = re.compile(r"pub (?:fn|const|static|type|trait)\s+([A-Za-z_0-9]+)")
FIELD = re.compile(r"pub ([a-z0-9_]+)\s*:\s*[^,;\n]+,")


def sources(root):
    path = Path(root)
    files = [path] if path.is_file() else sorted(p for p in path.rglob("*") if p.is_file())
    skip = {".git", "target", "vendor", "node_modules"}
    return [f for f in files if f.suffix in CODE and not skip & set(f.parts)]


def read(files):
    return {f: f.read_text(encoding="utf-8", errors="ignore") for f in files}


def suppressed(text):
    return any(ALLOW in line for line in text.splitlines())


def css_leads(reported):
    leads = []
    for path, text in reported.items():
        if path.suffix != ".css" or suppressed(text):
            continue
        for selector, body in RULE.findall(text):
            seen = set()
            for decl in DECL.finditer(body):
                prop = decl.group(1).strip().lower()
                if prop in seen:
                    leads.append((path, "duplicate-declaration", prop, selector.strip()[:48]))
                seen.add(prop)
    whole = "\n".join(reported.values())
    for path, text in reported.items():
        if path.suffix != ".css" or suppressed(text):
            continue
        for prop in PROP.findall(text):
            if f"var({prop}" not in whole and f"var( {prop}" not in whole:
                leads.append((path, "never-consumed", prop, "custom property"))
    return leads


def rust_leads(reported, whole):
    leads = []
    for path, text in reported.items():
        if path.suffix != ".rs" or suppressed(text):
            continue
        for name in PUB.findall(text):
            if name in {"new", "default"} or len(name) < 4:
                continue
            other = re.compile(rf"[^a-zA-Z0-9_]{re.escape(name)}\b")
            hits = sum(1 for f, t in reported.items() if f != path and other.search(t))
            if hits == 0:
                leads.append((path, "unreferenced-pub-item", name, "no other file names it"))
        if "settings" not in str(path) and "Settings" not in text:
            continue
        for field in FIELD.findall(text):
            reads = len(re.findall(rf"(?:with|map|get|try_get|with_untracked)[^;]*\.{field}\b", whole))
            reads += len(re.findall(rf"\b{singular(path)}\.{field}\b", whole))
            writes = len(re.findall(rf"\.{field}\s*=[^=]", whole))
            if reads == 0 and writes > 0:
                leads.append((path, "write-only-field", field, "written, never read"))
    return leads


def singular(path):
    return {"workspace.rs": "workspace", "layout.rs": "layout"}.get(path.name, "s")


def self_test():
    css = ".a { color: red; color: blue; }\n:root { --gone-x: 1px; }\n"
    rust = "pub fn never_called_from_anywhere() {}\npub const ALSO_UNTOUCHED: u8 = 1;\n"
    reported = {Path("a.css"): css, Path("b.rs"): rust}
    leads = css_leads(reported) + rust_leads(reported, css + rust)
    kinds = {lead[1] for lead in leads}
    details = {lead[2] for lead in leads}
    assert {"duplicate-declaration", "never-consumed", "unreferenced-pub-item"} <= kinds, kinds
    assert {"color", "--gone-x"} <= details, details
    print("dead-check: self-test ok")


def main():
    parser = argparse.ArgumentParser(description="report leads on code that cannot matter")
    parser.add_argument("root", nargs="?", default=".")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--limit", type=int, default=40)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    files = sources(args.root)
    reported = read(files)
    whole = "\n".join(reported.values())
    leads = css_leads(reported) + rust_leads(reported, whole)
    if args.json:
        print(json.dumps([{"file": str(f), "check": c, "detail": d, "where": w} for f, c, d, w in leads]))
    else:
        for path, check, detail, where in leads[: args.limit]:
            print(f"{path}: {check}: {detail}  [{where}]")
        print(f"\n{len(leads)} lead(s), {len(files)} file(s) read.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

# only the changed file was rewritten
