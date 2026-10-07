// DOM-contract sync check: names the engine queries must be the app's.

import { read, walk } from "./repo.js";

const ALL_FILES = walk(".");

const APP_TABLE = "crates/app-state/src/dom_contract.rs";
const ENGINE_TABLE = "public/engine/dom-contract.ts";
const ID_BUILDERS = "crates/reader-runtime/src/components/viewer/page_host.rs";

const RUST_SOURCES = ALL_FILES.filter(
  (file) =>
    file.endsWith(".rs") &&
    (file.startsWith("src/") || file.startsWith("crates/") || file.startsWith("src-tauri/")),
);

const ENGINE_SOURCES = ALL_FILES.filter((file) => file.endsWith(".ts") && file.startsWith("public/"));

type Table = Map<string, string>;

function parseAppTable(): Table {
  const text = read(APP_TABLE);
  const out: Table = new Map();
  const row = /pub const (\w+)\s*:\s*&str\s*=\s*"([^"]+)"/g;
  for (let m = row.exec(text); m; m = row.exec(text)) out.set(m[1]!, m[2]!);
  if (out.size === 0) throw new Error(`${APP_TABLE}: no contract constants found`);
  return out;
}

function parseEngineTable(): Table {
  const text = read(ENGINE_TABLE);
  const out: Table = new Map();
  const row = /export const (\w+)\s*=\s*"([^"]+)"/g;
  for (let m = row.exec(text); m; m = row.exec(text)) out.set(m[1]!, m[2]!);
  if (out.size === 0) throw new Error(`${ENGINE_TABLE}: no contract constants found`);
  return out;
}

const app = parseAppTable();
const engine = parseEngineTable();

/** What kind of name a constant is, decided by how it is named. */
type Kind = "attr" | "class" | "prefix" | "suffix" | "value";

function kindOf(name: string): Kind {
  if (name.endsWith("_ATTR")) return "attr";
  if (name.endsWith("_CLASS")) return "class";
  if (name.startsWith("ID_PREFIX_")) return "prefix";
  if (name.endsWith("_SUFFIX")) return "suffix";
  return "value";
}

const problems: string[] = [];

const RUST_TEXT = new Map<string, string>();
for (const file of RUST_SOURCES) RUST_TEXT.set(file, read(file));

// The engine may not invent names; the app owns the vocabulary.

for (const [name, value] of engine) {
  // An id fragment is assembled by `format!`; section 2 reads the builder.
  if (kindOf(name) === "prefix" || kindOf(name) === "suffix") continue;
  const declared = app.get(name);
  if (declared !== undefined) {
    if (declared !== value) {
      problems.push(`${ENGINE_TABLE}: ${name} is "${value}", but ${APP_TABLE} says "${declared}"`);
    }
    continue;
  }
  // A view-macro literal must be written by some host, or the query is empty.
  const written =
    kindOf(name) === "attr"
      ? [...RUST_TEXT.values()].some((text) => text.includes(`${value}=`))
      : [...RUST_TEXT.values()].some((text) => text.includes(`"${value}"`));
  if (!written) {
    problems.push(
      `${ENGINE_TABLE}: ${name} is "${value}", which no Rust source writes — ` +
        `the engine is reading an attribute the app never sets`,
    );
  }
}

// A Rust constant nothing references is how the two tables start to disagree.
for (const name of app.keys()) {
  const used = RUST_SOURCES.some(
    (file) => file !== APP_TABLE && new RegExp(`\\b${name}\\b`).test(RUST_TEXT.get(file) ?? ""),
  );
  if (!used) {
    problems.push(`${APP_TABLE}: ${name} is declared but nothing in the app references it`);
  }
}

// 2. The element-id shapes, read out of the builders.

function fnBody(file: string, fnName: string): string {
  const text = read(file);
  const start = text.indexOf(`fn ${fnName}`);
  if (start < 0) throw new Error(`${file}: no function ${fnName}`);
  const end = text.indexOf("\n}", start);
  if (end < 0) throw new Error(`${file}: cannot find the end of ${fnName}`);
  return text.slice(start, end);
}

function templates(body: string): string[] {
  const out: string[] = [];
  const row = /format!\("([^"]+)"/g;
  for (let m = row.exec(body); m; m = row.exec(body)) out.push(m[1]!);
  return out;
}

/** `sp-{page}-pg` -> the prefix and suffix a builder emits. */
function shape(template: string): { prefix: string; suffix: string } | null {
  const m = /^([a-z]+)-\{[^}]*\}(-[a-z]+)$/.exec(template);
  return m ? { prefix: m[1]!, suffix: m[2]! } : null;
}

const prefixes = new Set(
  [...engine].filter(([name]) => kindOf(name) === "prefix").map(([, value]) => value),
);
const suffix = (name: string): string | undefined => engine.get(name);

const HOST_SUFFIX = suffix("HOST_ID_SUFFIX");
const CANVAS_SUFFIX = suffix("CANVAS_ID_SUFFIX");
const WRAP_SUFFIX = suffix("STREAM_WRAP_SUFFIX");
const STREAM_PREFIX = suffix("ID_PREFIX_STREAM");
for (const [name, value] of [
  ["HOST_ID_SUFFIX", HOST_SUFFIX],
  ["CANVAS_ID_SUFFIX", CANVAS_SUFFIX],
  ["STREAM_WRAP_SUFFIX", WRAP_SUFFIX],
  ["ID_PREFIX_STREAM", STREAM_PREFIX],
] as const) {
  if (!value) throw new Error(`${ENGINE_TABLE}: ${name} is missing from the table`);
}

// The four page hosts. Each mode gets one id, all with the same suffix, and the
// engine declares every prefix the app can emit — an undeclared one is a mode
// whose ids the engine cannot parse.
const hostTemplates = templates(fnBody(ID_BUILDERS, "host_id_for_mode")).map(shape);
if (hostTemplates.length < 3) {
  throw new Error(`${ID_BUILDERS}: host_id_for_mode no longer builds one id per mode`);
}
for (const built of hostTemplates) {
  if (!built) throw new Error(`${ID_BUILDERS}: a host id is not built as prefix-page-suffix`);
  if (built.suffix !== HOST_SUFFIX) {
    problems.push(
      `${ID_BUILDERS}: host ids end in "${built.suffix}", but ${ENGINE_TABLE} says "${HOST_SUFFIX}"`,
    );
  }
  if (!prefixes.has(built.prefix)) {
    problems.push(
      `${ID_BUILDERS}: host ids start with "${built.prefix}-", which ${ENGINE_TABLE} does not declare`,
    );
  }
}
for (const declared of prefixes) {
  if (!hostTemplates.some((built) => built && built.prefix === declared)) {
    problems.push(
      `${ENGINE_TABLE}: ID_PREFIX_* declares "${declared}", but ${ID_BUILDERS} builds no such host id`,
    );
  }
}

// The canvas id is the host id with a different suffix, and the engine has to
// be able to walk both ways along that pair.
const swap = /replacen\("([^"]+)",\s*"([^"]+)",\s*1\)/.exec(fnBody(ID_BUILDERS, "canvas_id_for_mode"));
if (!swap) throw new Error(`${ID_BUILDERS}: canvas_id_for_mode no longer swaps one suffix for another`);
if (swap[1] !== HOST_SUFFIX || swap[2] !== CANVAS_SUFFIX) {
  problems.push(
    `${ID_BUILDERS}: canvas ids are the host id with "${swap[1]}" replaced by "${swap[2]}", ` +
      `but ${ENGINE_TABLE} says "${HOST_SUFFIX}" -> "${CANVAS_SUFFIX}"`,
  );
}

// The strip's wrapper rows, which the engine walks up to when a selection lands
// in the gap between two pages.
const wrapTemplates = RUST_SOURCES.flatMap((file) =>
  templates(RUST_TEXT.get(file) ?? "")
    .map(shape)
    .filter((built): built is { prefix: string; suffix: string } => built !== null && built.suffix === WRAP_SUFFIX)
    .map((built) => ({ file, ...built })),
);
if (wrapTemplates.length === 0) {
  problems.push(`no Rust source builds a "${WRAP_SUFFIX}" row id that ${ENGINE_TABLE} declares`);
} else if (!wrapTemplates.some((built) => built.prefix === STREAM_PREFIX)) {
  problems.push(
    `${ENGINE_TABLE}: STREAM_WRAP_SELECTOR expects "${STREAM_PREFIX}-" rows, ` +
      `but the app builds ${wrapTemplates.map((built) => `"${built.prefix}-"`).join(", ")}`,
  );
}

// 3. Nothing outside the tables may spell a contract name: a literal works
// the day it is written and stops matching the day the table moves.

/** Strip line and block comments, respecting string literals. */
function stripComments(text: string): string {
  let out = "";
  let quote: string | null = null;
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!;
    const next = text[i + 1];
    if (quote) {
      out += c;
      if (c === "\\") {
        out += next ?? "";
        i++;
      } else if (c === quote) {
        quote = null;
      }
      continue;
    }
    if (c === '"' || c === "'" || c === "`") {
      quote = c;
      out += c;
      continue;
    }
    if (c === "/" && next === "/") {
      while (i < text.length && text[i] !== "\n") i++;
      out += "\n";
      continue;
    }
    if (c === "/" && next === "*") {
      const end = text.indexOf("*/", i + 2);
      const skipped = text.slice(i, end < 0 ? text.length : end + 2);
      // Keep the newlines the comment spanned, so a reported line is the line
      // the reader sees.
      out += "\n".repeat((skipped.match(/\n/g) ?? []).length);
      i = end < 0 ? text.length : end + 1;
      continue;
    }
    out += c;
  }
  return out;
}

/** Every contract name, from both tables, as the engine would have to spell it. */
const names = new Map<string, string>(app);
for (const [name, value] of engine) names.set(name, value);

const forbidden: { name: string; pattern: RegExp }[] = [];
for (const [name, value] of names) {
  const kind = kindOf(name);
  // A class name is a substring of the identifiers built from it
  // (`TEXT_LAYER_CLASS` -> `st.textLayerEl`) and a host value is a substring
  // of plenty of prose, so both are matched only inside a string literal —
  // the only place a selector or a value can live. A prefix is matched with
  // the hyphen that makes it a prefix.
  const pattern =
    kind === "class" || kind === "value"
      ? new RegExp(`["'\`]\\.?${escapeRe(value)}["'\`]`)
      : new RegExp(escapeRe(kind === "prefix" ? `${value}-` : value));
  forbidden.push({ name, pattern });
}

function escapeRe(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

for (const file of ENGINE_SOURCES) {
  if (file === ENGINE_TABLE) continue;
  const text = stripComments(read(file));
  for (const entry of forbidden) {
    const m = entry.pattern.exec(text);
    if (m) {
      const line = text.slice(0, m.index).split("\n").length;
      problems.push(
        `${file}:${line}: ${m[0]} is a raw DOM-contract name (${entry.name}) — ` +
          `import it from ${ENGINE_TABLE}`,
      );
    }
    entry.pattern.lastIndex = 0;
  }
}

// 4. Every name the engine declares must be one it uses.

const ENGINE_TEXT = new Map<string, string>();
for (const file of ENGINE_SOURCES) ENGINE_TEXT.set(file, stripComments(read(file)));
const tableText = ENGINE_TEXT.get(ENGINE_TABLE) ?? "";

for (const name of engine.keys()) {
  const own = new RegExp(`\\b${name}\\b`, "g");
  const declaredHere = (tableText.match(own) ?? []).length;
  // One occurrence is the declaration; more means the table's own code uses it.
  if (declaredHere > 1) continue;
  const elsewhere = ENGINE_SOURCES.some(
    (file) => file !== ENGINE_TABLE && new RegExp(`\\b${name}\\b`).test(ENGINE_TEXT.get(file) ?? ""),
  );
  if (!elsewhere) {
    problems.push(`${ENGINE_TABLE}: ${name} is declared but the engine never uses it`);
  }
}

if (problems.length > 0) {
  console.error("::error::The DOM contract disagrees:");
  for (const problem of problems) console.error(`  ${problem}`);
  console.error("");
  console.error(`  ${APP_TABLE} is the app's half; ${ENGINE_TABLE} is the engine's.`);
  process.exit(1);
}

console.log(
  `dom contract agrees: ${engine.size} names in the engine, ${app.size} in the app, ` +
    `${hostTemplates.length} host-id shapes, ${wrapTemplates.length} wrapper rows`,
);
