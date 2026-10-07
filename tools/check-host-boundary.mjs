// The host speaks to panes only through host/contract.rs, never the store.

import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
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

/** Store reads the reader may make; other names go through `ShellApi`. */
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

/** The file's code with line comments blanked, line numbers preserved. */
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
    if (/\breload_window\b/.test(line)) {
      failures.push(`${at}: the window is the Shell's — reader code never reloads it\n    ${line.trim()}`);
    }
    if (substitute) return;
    for (const match of line.matchAll(/\bstorage::(\w+)/g)) {
      if (!STORAGE_READS.has(match[1])) {
        failures.push(
          `${at}: storage::${match[1]} is not a read — durable writes are the Shell's (ShellApi)\n    ${line.trim()}`,
        );
      }
    }
  });
}

// Retired execution paths are forbidden, not left as dormant alternatives.
const retiredWindowBridge = join(ROOT, "crates/app-ui/src/window_bridge.rs");
if (existsSync(retiredWindowBridge)) failures.push("orphaned window bridge returned; titlebar owns the scoped window-state module");
const wirePath = join(RUNTIME, "pane_wire.rs");
const panePath = join(RUNTIME, "frame_pane/mod.rs");
for (const file of [wirePath, panePath]) {
  for (const line of codeLines(file)) {
    if (/HostToPane::Open|\bOpenDelivery\b|\bdocument_started\b/.test(line)) {
      failures.push(`${relative(ROOT, file)}: retired in-realm/empty-realm document reuse returned`);
    }
  }
}
const titlebar = join(ROOT, "crates/app-ui/src/components/shell/titlebar/app_title_bar.rs");
if (!codeLines(titlebar).some((line) => line.includes("super::window_state::install"))) {
  failures.push("the live AppTitleBar does not install its scoped native window-state updater");
}
const commandTrait = join(ROOT, "crates/runtime-contract/src/boundary.rs");
if (codeLines(commandTrait).some((line) => /fn resolve_launch/.test(line))) {
  failures.push("ShellApi contains a synchronous launch query; hosted requests must return an awaited transport future");
}

if (failures.length > 0) {
  console.error(`host boundary: ${failures.length} violation(s)\n`);
  for (const failure of failures) console.error(`  ${failure}`);
  process.exit(1);
}
console.log(
  `host boundary: ${hostFiles.length} host sources clean; no legacy ReaderPage; ` +
    "reader writes go through ShellApi",
);
