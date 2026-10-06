#!/usr/bin/env python3
"""Refuse a commit whose subject is longer than the repository allows.

AGENTS.md fixes the subject at 72 characters, `[skip deep]` counted. This turns
that line into a gate instead of a hope.

  python3 tools/commit_check.py --install          # wire into .git/hooks
  python3 tools/commit_check.py --uninstall
  python3 tools/commit_check.py --range main..HEAD # audit committed history
  python3 tools/commit_check.py <file>             # hook mode: check a message

Exit 0 when every subject passes; in hook mode a non-zero exit is what refuses
the commit. Only the length is enforced here — the imperative mood, the lower
case and the rest of the subject rules stay with the reviewer.

A rebase runs the hook against the tree being rewritten, so a scripted reword of
history older than this file needs `git commit --amend --no-verify`, with
`--range` auditing the result.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys

LIMIT = 72
HOOK_MARKER = "mareader: commit subject length gate"
HOOK_BODY = f"""#!/bin/sh
# {HOOK_MARKER} — see tools/commit_check.py.
exec python3 "$(git rev-parse --show-toplevel)/tools/commit_check.py" "$1"
"""


def toplevel() -> str:
    out = subprocess.run(["git", "rev-parse", "--show-toplevel"],
                         capture_output=True, text=True, check=True)
    return out.stdout.strip()


def subject_of(message: str) -> str:
    """The first real line of a commit message: comments and blanks are scaffolding."""
    for line in message.splitlines():
        if line.startswith("#") or not line.strip():
            continue
        return line.rstrip()
    return ""


def too_long(subject: str) -> int:
    return max(0, len(subject) - LIMIT)


def check_subject(subject: str, label: str, stream) -> bool:
    over = too_long(subject)
    if not subject:
        print(f"{label}: no subject line — a commit message starts with one.",
              file=stream)
        return False
    if not over:
        return True
    print(f"{label}: subject is {len(subject)} characters, {over} over the "
          f"{LIMIT}-character limit:", file=stream)
    print(f"    {subject}", file=stream)
    return False


def hook(path: str) -> int:
    with open(path, encoding="utf-8") as handle:
        subject = subject_of(handle.read())
    if check_subject(subject, "commit refused", sys.stderr):
        return 0
    print("The subject names the change; the detail belongs in the body, after a "
          "blank line, where length is free.", file=sys.stderr)
    return 1


def audit(rev_range: str) -> int:
    out = subprocess.run(["git", "log", f"--format=%h%x09%s", rev_range],
                         capture_output=True, text=True, check=True)
    rows = [line.split("\t", 1) for line in out.stdout.splitlines() if line.strip()]
    bad = 0
    for sha, subject in rows:
        if not check_subject(subject, sha, sys.stdout):
            bad += 1
    print(f"{len(rows) - bad} of {len(rows)} subjects within {LIMIT} characters.")
    return 1 if bad else 0


def hook_path() -> str:
    git_dir = subprocess.run(["git", "rev-parse", "--absolute-git-dir"],
                             capture_output=True, text=True, check=True).stdout.strip()
    return os.path.join(git_dir, "hooks", "commit-msg")


def install(force: bool) -> int:
    path = hook_path()
    if os.path.exists(path) and HOOK_MARKER not in open(path, encoding="utf-8").read():
        if not force:
            print(f"{path} exists and is not ours; pass --force to replace it.")
            return 1
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(HOOK_BODY)
    os.chmod(path, 0o755)
    print(f"installed {path}")
    print("every commit now passes tools/commit_check.py before it is written.")
    return 0


def uninstall() -> int:
    path = hook_path()
    if not os.path.exists(path):
        print("no commit-msg hook is installed.")
        return 0
    if HOOK_MARKER not in open(path, encoding="utf-8").read():
        print(f"{path} is not ours; leaving it alone.")
        return 1
    os.unlink(path)
    print(f"removed {path}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("message_file", nargs="?",
                    help="the file git hands the hook (hook mode)")
    ap.add_argument("--range", dest="rev_range", metavar="REV",
                    help="audit every subject in a git range, e.g. main..HEAD")
    ap.add_argument("--install", action="store_true", help="write .git/hooks/commit-msg")
    ap.add_argument("--uninstall", action="store_true", help="remove that hook")
    ap.add_argument("--force", action="store_true",
                    help="with --install, replace a commit-msg hook we do not own")
    args = ap.parse_args()

    if args.install:
        return install(args.force)
    if args.uninstall:
        return uninstall()
    if args.rev_range:
        os.chdir(toplevel())
        return audit(args.rev_range)
    if args.message_file:
        return hook(args.message_file)
    subject = subject_of(sys.stdin.read()) if not sys.stdin.isatty() else ""
    if not subject:
        print("nothing to check: pass a message file, --range, or a message on stdin.")
        return 1
    return 0 if check_subject(subject, "stdin", sys.stdout) else 1


if __name__ == "__main__":
    sys.exit(main())

# only the changed file was rewritten
