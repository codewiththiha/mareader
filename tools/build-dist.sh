#!/usr/bin/env sh
# The canonical frontend build: ONE command that produces the shell page and
# every runtime artifact the shell dynamically imports, merged into dist/.
#
# This is the single source of truth for the frontend artifact set. Tauri's
# build command runs it (via `npm run build:dist`), CI runs it, the dev
# orchestrator (tools/dev.mjs) runs it, and the browser suites serve what it
# writes. The blank-window regression was two builds for one responsibility —
# CI ran this script while Tauri ran a bare `trunk build`, so the packaged app
# carried a shell page with no runtimes — so there is exactly one build here.
#
# Trunk's own hooks (engine bundle, tools/*.ts -> scripts/*.js, Tailwind CSS)
# fire inside each invocation, so no step below repeats them.
set -e

# Run from the repo root regardless of the caller's cwd: Tauri may invoke this
# from src-tauri/, and every path below is repo-relative.
cd "$(dirname "$0")/.."

# One layout owns all five targets and the required, deterministic merge.
node tools/runtime-artifacts.mjs --build "$@"

# The build is not done until the artifact set it promises is actually there:
# every runtime artifact present and non-empty, and the shell page carrying the
# boot placeholder the user sees before any runtime mounts. A missing artifact
# fails HERE, with the path in the message — never as a blank window later.
node tools/check-runtime-artifacts.mjs

echo "dist merged:"
ls dist/ | head -30
