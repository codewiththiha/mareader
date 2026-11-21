#!/usr/bin/env python3
"""Merge per-architecture PKGBUILD.<name> files into one multi-arch PKGBUILD.

cargo-packager emits a single-architecture `PKGBUILD` per Linux build.
The installers workflow saves each runner's file as `PKGBUILD.<matrix.name>`
so artifact merging does not overwrite one with another. This script combines
them into one Arch Linux multi-arch `PKGBUILD` using `source_<arch>` and
`sha512sums_<arch>`, verifies every archive is present in the directory, and
removes the per-arch `PKGBUILD.*` inputs.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ARCH_RE = re.compile(r"^arch=\('([^']+)'\)\s*$")
SOURCE_RE = re.compile(r'^source=\("([^"]+)"\)\s*$')
SHA512_RE = re.compile(r'^sha512sums=\("([0-9a-fA-F]+)"\)\s*$')
ARCH_ORDER = {"x86_64": 0, "aarch64": 1}


def parse_pkgbuild(path: Path) -> tuple[str, str, str, list[str]]:
    arch = source = sha512 = None
    skeleton: list[str] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if m := ARCH_RE.match(line):
            arch = m.group(1)
            skeleton.append("__ARCH__")
        elif m := SOURCE_RE.match(line):
            source = m.group(1)
            skeleton.append("__SOURCE__")
        elif m := SHA512_RE.match(line):
            sha512 = m.group(1)
            skeleton.append("__SHA512__")
        else:
            skeleton.append(line.rstrip())
    if not (arch and source and sha512):
        raise SystemExit(f"{path}: missing arch, source, or sha512sums line")
    return arch, source, sha512, skeleton


def merge(dist: Path) -> None:
    inputs = sorted(dist.glob("PKGBUILD.*"))
    if not inputs:
        if (dist / "PKGBUILD").is_file():
            return
        raise SystemExit(f"{dist}: no PKGBUILD.* files found")

    entries: list[tuple[str, str, str]] = []
    base_skeleton: list[str] | None = None
    for path in inputs:
        arch, source, sha512, skeleton = parse_pkgbuild(path)
        if base_skeleton is None:
            base_skeleton = skeleton
        elif skeleton != base_skeleton:
            raise SystemExit(f"{path}: PKGBUILD metadata differs from {inputs[0]}")
        if not (dist / source).is_file():
            raise SystemExit(f"{path}: referenced archive {source} missing from {dist}")
        entries.append((arch, source, sha512))

    entries.sort(key=lambda item: (ARCH_ORDER.get(item[0], 99), item[0]))
    seen = [arch for arch, _, _ in entries]
    if len(set(seen)) != len(seen):
        raise SystemExit(f"{dist}: duplicate PKGBUILD architecture in {seen}")

    assert base_skeleton is not None
    out: list[str] = []
    for line in base_skeleton:
        if line == "__ARCH__":
            arches = " ".join(f"'{arch}'" for arch, _, _ in entries)
            out.append(f"arch=({arches})")
        elif line == "__SOURCE__":
            if len(entries) == 1:
                _, source, sha512 = entries[0]
                out.append(f'source=("{source}")')
                out.append(f'sha512sums=("{sha512}")')
            else:
                for arch, source, sha512 in entries:
                    out.append(f'source_{arch}=("{source}")')
                    out.append(f'sha512sums_{arch}=("{sha512}")')
        elif line == "__SHA512__":
            continue
        else:
            out.append(line)

    (dist / "PKGBUILD").write_text("\n".join(out) + "\n", encoding="utf-8")
    for path in inputs:
        path.unlink()


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        raise SystemExit("usage: tools/merge-pkgbuild.py DIST_DIR")
    merge(Path(argv[1]))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
