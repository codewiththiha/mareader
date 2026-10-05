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

# One dist/, one writer. The shell leg below is a `trunk build` straight into
# dist/ — the very directory `trunk serve` owns and swaps wholesale at the end
# of ITS builds — so a build started while a dev server is up races it: both
# processes `remove_dir_all(dist)` and the loser dies with "error cleaning
# final dist: No such file or directory", which reads as a broken build.
# tools/dev.mjs sets MAREADER_DEV_BUILD=1 for the builds IT owns (its start-up
# build runs after it clears a stale server, and its rebuild loop deliberately
# runs beside its own `trunk serve`); every other caller is a second writer and
# is refused here, before Trunk has touched anything. The port is the one
# Trunk.toml serves on. MAREADER_DEV_PORT exists so the gate can be tested on
# a spare port without a dev server.
if [ -z "$MAREADER_DEV_BUILD" ]; then
  if node -e '
    const fs = require("node:fs"), net = require("node:net");
    const m = /\[serve\][\s\S]*?\bport\s*=\s*(\d+)/.exec(fs.readFileSync("Trunk.toml", "utf8"));
    const port = Number(process.env.MAREADER_DEV_PORT || (m ? m[1] : 1420));
    const sock = net.connect({ port, host: "127.0.0.1" });
    sock.setTimeout(750, () => { sock.destroy(); process.exit(1); });
    sock.on("connect", () => { sock.destroy(); process.exit(0); });
    sock.on("error", () => process.exit(1));
  '; then
    echo "build-dist.sh: a dev server is serving dist/ on the Trunk port — stop it first." >&2
    echo "  Two writers in one dist/ is the 'error cleaning final dist' failure." >&2
    echo "  Stop the dev server, or let it own the build: npm run dev" >&2
    exit 1
  fi
fi

# One layout owns all five targets and the required, deterministic merge.
node tools/runtime-artifacts.mjs --build "$@"

# The build is not done until the artifact set it promises is actually there:
# every runtime artifact present and non-empty, and the shell page carrying the
# boot placeholder the user sees before any runtime mounts. A missing artifact
# fails HERE, with the path in the message — never as a blank window later.
node tools/check-runtime-artifacts.mjs

echo "dist merged:"
ls dist/ | head -30
