// Trunk builds the host. The post_build hook (tools/build-formats.mjs) emits
// the three format artifacts into Trunk's staging dir, which becomes
// dist/formats. Not a second Trunk target. The dist directory does not move.
//
// Windows can run this; a shell script cannot be Tauri's beforeBuildCommand.
//
// A missing formats/pdf.js must not be the app page. trunk serve falls back
// to index.html for a path it does not have, and importing that page is
// `Unexpected token '<'` in a blob the debugger names `source`.

import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit" });
  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

run("trunk", ["build", "--release"]);
