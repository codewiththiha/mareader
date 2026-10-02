// The reader host's dependency boundary, asserted from its source.
//
// The workspace is split three ways inside the reader runtime:
//
//   crates/reader-runtime/src/host/**   the reader host: chrome placement,
//                                       focus, bounds, pane create/remove,
//                                       workspace commands, lifecycle dispatch
//   crates/reader-runtime/src/pane/**   the production pane: one document
//                                       session, format rendering, virtualizers
//   crates/reader-runtime/src/lib.rs    the composition root that hands the
//                                       host its pane factory
//
// and the dependency direction is `host → contract → format`. The host
// speaks to panes only through `host/contract.rs`; it must never name a
// format, an engine type, the pane implementation, or a pane's reader
// state. The compiler cannot say that (they share a crate), so this check
// does: every non-comment line under `host/` is scanned for the names that
// would break the direction.
//
// A second rule keeps the legacy path removed: the old `ReaderPage`
// workspace monolith must not come back as a name anywhere in the reader
// runtime's code.
//
// A third keeps the Shell's side of the boundary the Shell's: durable
// persistence and the window belong to the Shell, so reader code reaches
// them only through `ShellApi`. Outside `context.rs` (whose `StandaloneApi`
// stands in for the Shell when there is none) the reader may READ the
// origin's store — the allowlist below — but never write it, and never
// reload the window itself.
//
// Plain node modules only, so it runs in a lane that has not run `npm ci`.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname;
const RUNTIME = join(ROOT, "crates/reader-runtime/src");
const HOST = join(RUNTIME, "host");

/** Names the host may not use, each with the reason a reviewer reads. */
const HOST_FORBIDDEN = [
  [/\bpdf_engine\b/, "the PDF engine is a format concern (pane side)"],
  [/\bpdf_core\b/, "PDF geometry is a format concern (pane side)"],
  [/\bDocStatus\b/, "the engine's status type: the host reads PaneDocStatus"],
  [/\bcrate::components::formats\b/, "format renderers belong to the pane"],
  [/\bcrate::pane\b/, "the host never names the pane implementation (the factory is injected)"],
  [/\bcrate::state::/, "a pane's reader state is the pane's"],
  [/\bcrate::services\b/, "document services run inside a pane"],
  [/\bcrate::effects\b/, "reader effects are installed by a pane"],
  [/\bcrate::zoom\b/, "zoom is a pane's viewport geometry"],
  [/\bcrate::features\b/, "reader feature trees are filled in by a pane"],
  [/\bvirtual_list(_leptos)?\b/, "virtualizers are pane resources"],
  [/\bReaderContext\b/, "a pane's context: the host holds none"],
  [/\bReaderState\b/, "a pane's state: the host holds none"],
  [/\breader_core::(format|view|reflow)\b/, "document-format types belong to the pane"],
];

/** The store reads reader code may make itself; every other `storage::`
 *  name is a write (or a new function nobody has classified yet) and goes
 *  through `ShellApi`. An allowlist, so a new writer fails closed. */
const STORAGE_READS = new Set([
  "get",
  "load_settings",
  "load_library",
  "load_covers",
  "load_gloss",
  "library_stamp",
  "covers_stamp",
  "settings_stamp",
  "encode_gloss",
  "resolve_launch",
]);

/** The one file allowed to write: the Shell-less `StandaloneApi`. */
const SHELL_SUBSTITUTE = join(RUNTIME, "context.rs");

/** `crate::context::` is allowed for the boundary handle only. */
const CONTEXT_ALLOWED = new Set(["ApiHandle"]);

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...walk(path));
    else if (name.endsWith(".rs")) out.push(path);
  }
  return out;
}

/** The file's code with line comments (`//`, `///`, `//!`) blanked out,
 *  line numbers preserved. String literals are left alone: none of the
 *  forbidden names belongs in a host string either. */
function codeLines(path) {
  return readFileSync(path, "utf8")
    .split("\n")
    .map((line) => {
      const at = line.indexOf("//");
      return at === -1 ? line : line.slice(0, at);
    });
}

const failures = [];

let hostFiles;
try {
  hostFiles = walk(HOST);
} catch {
  failures.push(`${relative(ROOT, HOST)}: the host module is missing`);
  hostFiles = [];
}
if (hostFiles.length === 0 && failures.length === 0) {
  failures.push(`${relative(ROOT, HOST)}: no host sources found`);
}

for (const file of hostFiles) {
  codeLines(file).forEach((line, i) => {
    for (const [pattern, reason] of HOST_FORBIDDEN) {
      if (pattern.test(line)) {
        failures.push(`${relative(ROOT, file)}:${i + 1}: ${reason}\n    ${line.trim()}`);
      }
    }
    for (const match of line.matchAll(/\bcrate::context::(\w+)/g)) {
      if (!CONTEXT_ALLOWED.has(match[1])) {
        failures.push(
          `${relative(ROOT, file)}:${i + 1}: the host reaches crate::context only for ApiHandle, not ${match[1]}\n    ${line.trim()}`,
        );
      }
    }
  });
}

for (const file of walk(RUNTIME)) {
  const substitute = file === SHELL_SUBSTITUTE;
  codeLines(file).forEach((line, i) => {
    const at = `${relative(ROOT, file)}:${i + 1}`;
    if (/\bReaderPage\b/.test(line)) {
      failures.push(
        `${at}: the legacy ReaderPage workspace owner is gone — the host and its panes own the reader\n    ${line.trim()}`,
      );
    }
    if (substitute) return;
    for (const match of line.matchAll(/\bstorage::(\w+)/g)) {
      if (!STORAGE_READS.has(match[1])) {
        failures.push(
          `${at}: storage::${match[1]} is not a read — durable writes are the Shell's (ShellApi)\n    ${line.trim()}`,
        );
      }
    }
    if (/\breload_window\b/.test(line)) {
      failures.push(
        `${at}: the window is the Shell's — ask with ShellApi::reload\n    ${line.trim()}`,
      );
    }
  });
}

if (failures.length > 0) {
  console.error(`host boundary: ${failures.length} violation(s)\n`);
  for (const failure of failures) console.error(`  ${failure}`);
  process.exit(1);
}
console.log(
  `host boundary: ${hostFiles.length} host sources clean; no legacy ReaderPage; ` +
    "reader writes and reloads go through ShellApi",
);
