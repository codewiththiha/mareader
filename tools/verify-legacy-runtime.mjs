#!/usr/bin/env node
// Static smoke gate for CI. It intentionally fails if the Tauri frontend
// still claims to be the old routed root application instead of the workspace.
import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const failures = [];

function read(file) {
  const p = path.join(root, file);
  if (!fs.existsSync(p)) {
    failures.push(`missing ${file}`);
    return "";
  }
  return fs.readFileSync(p, "utf8");
}

const host = read("host/host.ts");
const workspaceHost = read("host/workspace-host.ts");
const index = read("index.html");
const app = read("src/app/mod.rs");
const panes = read("host/panes.ts");
const readerPage = read("src/features/reader/page.rs");

if (!/workspace/i.test(workspaceHost)) {
  failures.push("workspace-host.ts does not contain workspace boot/runtime wiring");
}

if (/new HostRuntimeManager\b/.test(host)) {
  failures.push("host.ts still instantiates HostRuntimeManager; pane runtime manager must be authoritative");
}

if (/current\s*:\s*ReaderRuntime/.test(host)) {
  failures.push("host.ts still contains single-runtime ownership");
}

if (/AppTitleBar/.test(readerPage)) {
  failures.push("ReaderPage still owns AppTitleBar; reader iframe must be document-surface only");
}

if (/ReaderRail/.test(readerPage)) {
  failures.push("ReaderPage still owns ReaderRail; workspace must own sidebar");
}

if (!index.includes('apps/workspace-wasm/Cargo.toml') || !index.includes('id="workspace-root"')) failures.push("desktop root must build and mount workspace-wasm");
if (!host.includes("bootWorkspace()")) failures.push("host does not boot the workspace");
if (!app.includes("mount_workspace") || !app.includes("ReaderSurface")) failures.push("missing production WASM mounts");
if (!panes.includes("Map<string, ReaderRuntime>")) failures.push("missing per-pane runtime ownership");

if (failures.length) {
  console.error("LEGACY RUNTIME GATE FAILED");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

console.log("legacy runtime gate passed");
