// The repo-reading prelude the check tools share: resolve the repo root,
// read a file, walk the tree past the directories that are not source. Five
// tools each carried a byte-identical copy of these, and identical copies
// stay in step only until a new build directory appears and one script scans
// it while the others do not. Nothing here knows what any check is FOR; the
// parsing stays with the tools. Emitted to `scripts/repo.js` like everything
// else in this directory: tools/ is source, scripts/ is generated output
// (see tsconfig.tools.json).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Absolute path of the repository root — the directory holding package.json. */
export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** Every directory in the repo, minus the ones that are not source. */
const SKIP_DIRS = new Set([".git", "node_modules", "target", "dist", ".arena", "out", "build"]);

/** Resolve a repo-relative path against the root. An absolute path is
 *  returned unchanged, for a caller compiled into `scripts/` whose depth
 *  differs from its source's and which therefore anchors on `import.meta.url`
 *  instead. */
function resolvePath(rel: string): string {
  return path.isAbsolute(rel) ? rel : path.join(root, rel);
}

/** Read a repo-relative (or absolute) file as UTF-8. */
export function read(rel: string): string {
  return fs.readFileSync(resolvePath(rel), "utf8");
}

/** True when a repo-relative (or absolute) path is an existing regular file. */
export function isFile(rel: string): boolean {
  const abs = resolvePath(rel);
  return fs.existsSync(abs) && fs.statSync(abs).isFile();
}

/** The exported string constants a module declares, by name. `check-events`
 *  reads both event tables with it and the engine smoke reads the engine's, so
 *  a name the smoke asserts on comes from the table rather than from a third
 *  copy of it. `rel` is repo-relative, or absolute for a caller compiled into
 *  `scripts/` whose own depth differs from its source's. */
export function exportedStrings(rel: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const m of read(rel).matchAll(/export const (\w+)\s*=\s*"([^"]+)"/g)) {
    out.set(m[1]!, m[2]!);
  }
  return out;
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
