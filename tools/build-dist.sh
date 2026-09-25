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
#
# The built PAGE is the one file whose name the builder chooses: Trunk writes
# the target it built (reader.html / library.html) or, when it normalizes the
# output, index.html. Take whichever is there and fail when neither is — the
# old `|| true` silently shipped a dist with no standalone page for either
# runtime, which is the same class of silence the artifact check exists to end.
copy_page() {
  dir="$1"
  page="$2"
  if [ -f "$dir/$page" ]; then
    cp "$dir/$page" "dist/$page"
    return 0
  fi
  if [ -f "$dir/index.html" ]; then
    cp "$dir/index.html" "dist/$page"
    return 0
  fi
  # Last resort that cannot pick the wrong file: each runtime dist holds only
  # its own page, so any page in there is the one this build produced.
  built=$(ls "$dir"/*.html 2>/dev/null | head -1)
  if [ -n "$built" ] && [ -f "$built" ]; then
    cp "$built" "dist/$page"
    return 0
  fi
  echo "build-dist.sh: no built page in $dir for dist/$page" >&2
  return 1
}

copy_page dist-reader reader.html
cp dist-reader/reader.js dist/
cp dist-reader/reader_bg.wasm dist/
copy_page dist-library library.html
cp dist-library/library.js dist/
cp dist-library/library_bg.wasm dist/

# Both runtime pages carry their wasm too: without it the standalone page
# loads and cannot boot, which is the same failure one level down.
if [ ! -f dist/reader_bg.wasm ] || [ ! -f dist/library_bg.wasm ]; then
  echo "build-dist.sh: a runtime wasm module did not reach dist/" >&2
  exit 1
fi

# The build is not done until the artifact set it promises is actually there:
# every runtime artifact present and non-empty, and the shell page carrying the
# boot placeholder the user sees before any runtime mounts. A missing artifact
# fails HERE, with the path in the message — never as a blank window later.
node tools/check-runtime-artifacts.mjs

echo "dist merged:"
ls dist/ | head -30
