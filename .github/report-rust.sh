#!/usr/bin/env bash
# TEMPORARY developer aid — delete before merging.
#
# The token this branch is pushed with can read Actions metadata but cannot
# download job logs ("Must have admin rights to Repository"), so a red lane is
# a red square with no explanation attached. This script runs the same gates
# ci.yml runs and re-emits the first errors as workflow annotations, which the
# check-runs API does serve. Nothing here changes what CI gates on; it only
# makes a failure legible from the outside.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Escape a blob for the `::error::` command parser: percent first, then
# colons, then newlines — the order matters, because the escapes themselves
# contain both.
escape() {
  sed -e 's/%/%25/g' -e 's/:/%3A/g' "$1" | awk '{printf "%s%%0A", $0}'
}

# Print the interesting part of a tool's output as one annotation.
report() {
  local title="$1" file="$2"
  [ -s "$file" ] || return 0
  local blob
  blob=$(grep -nE "^(error|warning|failures:|test result: FAILED)" -A 10 "$file" | head -c 6000)
  if [ -z "$blob" ]; then
    blob=$(tail -c 1200 "$file")
  fi
  local tmp
  tmp="$(mktemp)"
  printf '%s' "$blob" > "$tmp"
  echo "::error title=${title}::$(escape "$tmp")"
  rm -f "$tmp"
}

status=0

echo "--- clippy (workspace, deny warnings) ---"
if ! cargo clippy --workspace --all-targets --exclude mareader-shell --locked --keep-going \
    -- -D warnings > /tmp/clippy.txt 2>&1; then
  status=1
  report "clippy" /tmp/clippy.txt
fi

echo "--- cargo check (wasm32) ---"
if ! cargo check --target wasm32-unknown-unknown --locked --keep-going > /tmp/wasm.txt 2>&1; then
  status=1
  report "wasm check" /tmp/wasm.txt
fi

echo "--- cargo test (workspace) ---"
if ! cargo test --workspace --exclude mareader-shell --locked --no-fail-fast \
    > /tmp/test.txt 2>&1; then
  status=1
  report "tests" /tmp/test.txt
fi

if [ "$status" -eq 0 ]; then
  actual=$(grep -oE '^test result: ok\. [0-9]+ passed' /tmp/test.txt \
    | grep -oE '[0-9]+' | awk '{s+=$1} END{print s}')
  echo "workspace ran $actual tests"
  claimed=$(grep -oE '^[[:space:]]*(- )?[0-9][0-9,]* (Rust )?tests?\b' README.md \
    | grep -oE '[0-9][0-9,]*' | tr -d ',' | head -1)
  if [ "$claimed" != "$actual" ]; then
    echo "::error title=test count::README claims ${claimed}, workspace ran ${actual}"
    status=1
  fi
  echo "--- summary ---"
  grep -E "^(test result|running [0-9]+ tests)" /tmp/test.txt | head -40
fi

exit "$status"
