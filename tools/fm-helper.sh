#!/usr/bin/env bash
# Builds the Apple Intelligence helper into apps/mareader/helper (macOS 26).
# Usage: SIGN_IDENTITY="Developer ID Application: …" tools/fm-helper.sh
set -euo pipefail
cd "$(dirname "$0")/.."
# The helper must speak the protocol of the fm-bridge crate we link.
rev=$(grep -o 'fm-bridge?rev=[0-9a-f]*' Cargo.lock | head -1 | cut -d= -f2)
[ -n "$rev" ] || { echo "error: fm-bridge is not in Cargo.lock" >&2; exit 1; }
src=.cache/ref/fm-bridge
if [ ! -d "$src/.git" ]; then
  git clone -q https://github.com/codewiththiha/fm-bridge.git "$src"
fi
git -C "$src" fetch -q origin "$rev"
git -C "$src" checkout -q --detach "$rev"
BUNDLE_ID="${BUNDLE_ID:-com.codewiththiha.mareader.FMBridge}" \
  "$src/scripts/build-helper.sh" "$PWD/apps/mareader/helper"
echo "built apps/mareader/helper/FMBridge from fm-bridge $rev"
