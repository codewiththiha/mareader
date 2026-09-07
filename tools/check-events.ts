// Window-event protocol sync check — the same cheap insurance as
// `check-versions.ts`, for the fact written down in two languages.
//
// The app and the imperative engine under `public/engine/` talk through
// CustomEvents on `window` (the engine is a bundled IIFE that cannot hold a
// Leptos signal and cannot be called from inside). The names cross that
// boundary declared twice: src/events.rs (the app's whole table, and the
// only place a Rust listener may take a name from) and
// public/engine/events.ts (the engine's dispatched half). A disagreement is
// not a compile error on either side — it is a dispatch into a window nobody
// listens on, and the only symptom is silence. This script fails CI when the
// tables drift, when a declared event lacks a dispatcher or listener, or
// when a literal bypasses the tables entirely.
//
// TypeScript source; Trunk's pre-build hook compiles it to
// `scripts/check-events.js`.

import { isFile, read, walk } from "./repo.js";

const ALL_FILES = walk(".");

const APP_TABLE = "src/events.rs";
const ENGINE_TABLE = "public/engine/events.ts";

// ---------------------------------------------------------------------------
// The two tables — parsed rather than imported, for the same reason as
// check-formats.ts: the app's table is a const in a wasm-targeted crate, and
// emitting JSON from it would be more machinery than the strings it guards.
// Both patterns throw rather than return empty when the shape moves, so a
// refactor cannot silently empty the check.
// ---------------------------------------------------------------------------

type Table = Map<string, string>;

function parseAppTable(): Table {
  const text = read(APP_TABLE);
  const out: Table = new Map();
  const row = /pub const (\w+)\s*:\s*&str\s*=\s*"([^"]+)"/g;
  for (let m = row.exec(text); m; m = row.exec(text)) out.set(m[1]!, m[2]!);
  if (out.size === 0) throw new Error(`${APP_TABLE}: no event constants found`);
  return out;
}

function parseEngineTable(): Table {
  const text = read(ENGINE_TABLE);
  const out: Table = new Map();
  const row = /export const (\w+)\s*=\s*"([^"]+)"/g;
  for (let m = row.exec(text); m; m = row.exec(text)) out.set(m[1]!, m[2]!);
  if (out.size === 0) throw new Error(`${ENGINE_TABLE}: no event constants found`);
  return out;
}

const app = parseAppTable();
const engine = parseEngineTable();

// ---------------------------------------------------------------------------
// Where a name may be referenced from, per table.
// ---------------------------------------------------------------------------

const RUST_SOURCES = ALL_FILES.filter(
  (file) =>
    file.endsWith(".rs") &&
    (file.startsWith("src/") || file.startsWith("crates/") || file.startsWith("src-tauri/")),
);

const ENGINE_SOURCES = ALL_FILES.filter(
  (file) => file.endsWith(".ts") && (file.startsWith("public/")),
);

/** Files scanned for a name's references, and for stray literals. */
const SCANNABLE = [...new Set([...RUST_SOURCES, ...ENGINE_SOURCES, "index.html"])].filter(isFile);

const TEXTS = new Map<string, string>();
for (const file of SCANNABLE) TEXTS.set(file, read(file));

const problems: string[] = [];

// ---------------------------------------------------------------------------
// 1. The two tables must agree on every name the engine declares.
// ---------------------------------------------------------------------------

for (const [name, value] of engine) {
  const declared = app.get(name);
  if (declared === undefined) {
    problems.push(
      `${ENGINE_TABLE}: ${name} is not in ${APP_TABLE} — the app cannot be listening for it`,
    );
  } else if (declared !== value) {
    problems.push(
      `${ENGINE_TABLE}: ${name} is "${value}", but ${APP_TABLE} says "${declared}"`,
    );
  }
}

// Two constants with the same string are one event under two names, which is
// how a rename starts and never finishes.
const seen = new Map<string, string>();
for (const [name, value] of app) {
  const other = seen.get(value);
  if (other) problems.push(`${APP_TABLE}: ${name} and ${other} are both "${value}"`);
  seen.set(value, name);
}

// ---------------------------------------------------------------------------
// 2. Every declared event must have both a dispatcher and a listener: an
// unused constant is a name written down and then forgotten, and the table
// advertises a protocol the app does not speak.
// ---------------------------------------------------------------------------

function referenced(name: string, files: string[], except: string): boolean {
  const pattern = new RegExp(`\\b${name}\\b`);
  return files.some((file) => file !== except && pattern.test(TEXTS.get(file) ?? ""));
}

for (const name of app.keys()) {
  if (!referenced(name, RUST_SOURCES, APP_TABLE)) {
    problems.push(`${APP_TABLE}: ${name} is declared but nothing in the app references it`);
  }
}

for (const name of engine.keys()) {
  if (!referenced(name, ENGINE_SOURCES, ENGINE_TABLE)) {
    problems.push(`${ENGINE_TABLE}: ${name} is declared but the engine never dispatches it`);
  }
}

// ---------------------------------------------------------------------------
// 3. No event name may be written as a literal anywhere but the two tables:
// a literal works the day it is written and stops matching the day the table
// moves, and looking at the table will not find it.
// ---------------------------------------------------------------------------

const LITERAL = /["']pdfreader:[A-Za-z0-9._-]+["']/g;

for (const [file, text] of TEXTS) {
  if (file === APP_TABLE || file === ENGINE_TABLE) continue;
  for (let m = LITERAL.exec(text); m; m = LITERAL.exec(text)) {
    const line = text.slice(0, m.index).split("\n").length;
    problems.push(`${file}:${line}: ${m[0]} is a raw event name — import it from ${
      file.endsWith(".rs") ? APP_TABLE : ENGINE_TABLE
    }`);
  }
  LITERAL.lastIndex = 0;
}

if (problems.length > 0) {
  console.error("::error::The window-event tables disagree:");
  for (const problem of problems) console.error(`  ${problem}`);
  console.error("");
  console.error(`  ${APP_TABLE} is the app's table; ${ENGINE_TABLE} declares the`);
  console.error(`  three the engine dispatches across to it.`);
  process.exit(1);
}

console.log(
  `events agree: ${[...engine.keys()].map((name) => `${name}=${app.get(name)}`).join(", ")} ` +
    `(${app.size} in ${APP_TABLE})`,
);
