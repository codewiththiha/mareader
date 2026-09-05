// The repo-reading prelude the check tools share.
//
// Five tools under `tools/` answer questions about this repository's own
// text: versions that have to agree, formats declared in three places, event
// names spelled in two languages, DOM names spelled in two, module paths named
// in comments. Each of them began the same way — resolve the repo root, read a
// file, walk the tree past the directories that are not source — and each
// carried its own byte-identical copy of those four things. Identical copies
// stay in step right up to the day a new build directory appears and one script
// starts scanning it while the others do not. This is the single copy.
//
// Nothing here knows what any check is FOR; the parsing stays with the tools.
//
// Emitted to `scripts/repo.js` like everything else in this directory: `tools/`
// is source, `scripts/` is generated output (see tsconfig.tools.json).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Absolute path of the repository root — the directory holding package.json. */
export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** Every directory in the repo, minus the ones that are not source. */
const SKIP_DIRS = new Set([".git", "node_modules", "target", "dist", ".arena", "out", "build"]);

/** Read a repo-relative file as UTF-8. */
export function read(rel: string): string {
  return fs.readFileSync(path.join(root, rel), "utf8");
}

/** True when a repo-relative path exists and is a regular file. */
export function isFile(rel: string): boolean {
  return fs.existsSync(path.join(root, rel)) && fs.statSync(path.join(root, rel)).isFile();
}

/** Every file under `dir` — repo-relative, posix separators, `SKIP_DIRS`
 *  excluded. Callers filter the result by extension and by top-level directory. */
export function walk(dir: string, out: string[] = []): string[] {
  for (const entry of fs.readdirSync(path.join(root, dir), { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue;
      walk(path.posix.join(dir, entry.name), out);
    } else {
      out.push(path.posix.join(dir, entry.name));
    }
  }
  return out;
}
