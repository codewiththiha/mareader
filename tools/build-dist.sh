#!/usr/bin/env sh
# The one frontend build: shell page + runtime artifacts, into dist/.
set -e

# Run from the repo root whatever the caller's cwd: every path is repo-relative.
cd "$(dirname "$0")/.."

# One dist/, one writer: a second build beside a live server is refused.
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

# Done when every promised artifact exists and the boot placeholder is present.
node tools/check-runtime-artifacts.mjs

echo "dist merged:"
ls dist/ | head -30
