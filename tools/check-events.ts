// Window-event protocol sync check — the same cheap insurance as
// `check-versions.ts` and `check-formats.ts`, for the fact that is written
// down in two languages.
//
// The app and the imperative engine under `public/engine/` talk through
// CustomEvents on `window`, because the engine is a bundled IIFE that cannot
// hold a Leptos signal and the app cannot be called from inside it. Three
// names cross that boundary. They are declared twice:
//
//   - src/events.rs                 the app's whole table, and the only place
//                                   a Rust listener may take a name from
//   - public/engine/events.ts       the engine's half: the three it dispatches
//
// A name that disagrees is not a compile error on either side. It is a
// dispatch into a window nobody is listening on: internal links stop
// navigating, or the selection pill stops appearing, and the only symptom is
// silence. Both tables exist so that neither side has to invent a string, and
// this script exists so that the strings cannot drift apart, or be bypassed by
// a literal that never joined a table.
//
// It also checks the tables are load-bearing: an event constant that nothing
// references is a name that was declared and then forgotten, which is the same
// quiet failure one step earlier.
//
// This is the TypeScript source; Trunk's pre-build hook compiles it to
// `scripts/check-events.js` so CI can run it with plain `node`.

import { isFile, read, walk } from "./repo.js";

const ALL_FILES = walk(".");

const APP_TABLE = "src/events.rs";
const ENGINE_TABLE = "public/engine/events.ts";

// ---------------------------------------------------------------------------
// The two tables.
// ---------------------------------------------------------------------------
// Parsed rather than imported, for the same reason as `check-formats.ts`: the
// app's table is a `const` in a wasm-targeted crate, and a build step that
// emitted JSON from it would be more machinery than the seven strings it
// guards. Both patterns throw rather than returning an empty list when the
// shape moves, so a refactor of either table cannot silently empty this check.

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
// 2. Every declared event must have both a dispatcher and a listener.
// ---------------------------------------------------------------------------
// An unused constant is a name that was written down and then forgotten: the
// event it stands for either never existed or stopped being wired up, and the
// table now advertises a protocol the app does not speak.

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
// 3. No event name may be written as a literal anywhere but the two tables.
// ---------------------------------------------------------------------------
// This is the rule that keeps the tables from becoming decoration. A literal
// compiles and works on the day it is written; it is the day the table moves
// that it stops matching, and it will not be found by looking at the table.

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
