#!/usr/bin/env sh
# The Phase 2 dist: the shell page (index.html) from the default config,
# then the two runtime pages into their own dists, merged into dist/ so one
# static server serves the app AND both standalone runtime pages.
set -e

trunk build "$@"
trunk build --config reader.Trunk.toml --dist dist-reader "$@"
trunk build --config library.Trunk.toml --dist dist-library "$@"

cp dist-reader/reader.html dist/ 2>/dev/null || true
cp dist-reader/reader.js dist/ 2>/dev/null || true
cp dist-reader/reader_bg.wasm dist/ 2>/dev/null || true
cp dist-library/library.html dist/ 2>/dev/null || true
cp dist-library/library.js dist/ 2>/dev/null || true
cp dist-library/library_bg.wasm dist/ 2>/dev/null || true
echo "dist merged:"
ls dist/ | head -30
