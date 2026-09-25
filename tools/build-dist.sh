#!/usr/bin/env sh
# The canonical frontend build: ONE command that produces the shell page and
# both runtime artifacts the shell dynamically imports, merged into dist/.
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

trunk build "$@"
trunk build --config reader.Trunk.toml --dist dist-reader "$@"
trunk build --config library.Trunk.toml --dist dist-library "$@"

# The merge is part of the build, not a convenience: a missing source here
# means the artifact set is broken, so these copies are allowed to fail loudly.
# A `|| true` on this step is how a missing runtime artifact stayed invisible.
cp dist-reader/reader.html dist/
cp dist-reader/reader.js dist/
cp dist-reader/reader_bg.wasm dist/
cp dist-library/library.html dist/
cp dist-library/library.js dist/
cp dist-library/library_bg.wasm dist/

# The build is not done until the artifact set it promises is actually there:
# every runtime artifact present and non-empty, and the shell page carrying the
# boot placeholder the user sees before any runtime mounts. A missing artifact
# fails HERE, with the path in the message — never as a blank window later.
node tools/check-runtime-artifacts.mjs

echo "dist merged:"
ls dist/ | head -30
