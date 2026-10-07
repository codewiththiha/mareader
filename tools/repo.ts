// The repo-reading prelude the check tools share: root, read, walk.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** The repository root: the directory holding package.json. */
export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** Every directory in the repo, minus the ones that are not source. */
const SKIP_DIRS = new Set([".git", "node_modules", "target", "dist", ".arena", "out", "build"]);

/** Resolve a repo-relative path against the root; absolute paths pass. */
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

/** The exported string constants a module declares, by name. */
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
