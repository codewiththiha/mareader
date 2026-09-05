// Doc-path check — the third piece of cheap insurance in this pipeline, after
// `check-versions.ts` and `check-formats.ts`.
//
// This repo's comments carry a lot of weight: module docs explain why a design
// is the way it is, and they name the other modules that make it work. Those
// names rot silently. A rename or a move leaves the prose pointing at a module
// that no longer exists, nothing fails, and the next reader follows the sign to
// an empty field — a `cargo doc` link would warn, but most of these are plain
// backticked prose, which no tool reads.
//
// So: every module path (`crate::a::b`, `super::x`, `ai_core::gloss::y`) and
// every file path with a slash in it (`effects/reader/zoom.rs`) that appears in
// a Rust comment must resolve, and so must every path the two prose documents
// put in backticks — README.md and ARCHITECTURE.md send readers to files, and
// those references rot the same way a comment's do. Resolution is deliberately shallow — it checks
// that the MODULE exists and that the last name is declared or re-exported
// there, which is all a comment promises — and deliberately conservative: a path
// whose first segment is not one of ours is assumed to be an external crate and
// skipped, and a module that re-exports with a glob passes whatever it is asked
// about.
//
// This is the TypeScript source; Trunk's pre-build hook compiles it to
// `scripts/check-doc-paths.js` so CI can run it with plain `node`.

import fs from "node:fs";
import path from "node:path";

import { isFile, read, root, walk } from "./repo.js";

const ALL_FILES = walk(".");

/** The Rust files whose comments are checked. */
const RUST_FILES = ALL_FILES.filter(
  (file) =>
    file.endsWith(".rs") &&
    (file.startsWith("src/") || file.startsWith("crates/") || file.startsWith("src-tauri/")),
);

// ---------------------------------------------------------------------------
// Crate roots, so `ai_core::gloss` knows which `src/` it starts from.
// ---------------------------------------------------------------------------
const CRATE_ROOTS = new Map<string, string>();
for (const cargo of ALL_FILES.filter((file) => file.endsWith("Cargo.toml"))) {
  if (cargo.includes("/tests/") || cargo.includes("target/")) continue;
  const name = /^name\s*=\s*"([^"]+)"/m.exec(read(cargo))?.[1];
  if (!name) continue;
  const dir = path.posix.dirname(cargo);
  const src = path.posix.join(dir, "src");
  if (fs.existsSync(path.join(root, src))) CRATE_ROOTS.set(name.replace(/-/g, "_"), src);
}

/**
 * First segments that are never ours: Rust's own primitives and the external
 * crates this workspace builds against. A path starting with anything else
 * unknown is skipped too — the check only speaks for names it can resolve.
 */
const SKIP_FIRST = new Set(
  `i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 usize isize bool char str
   std core alloc serde serde_json leptos web_sys js_sys wasm_bindgen tauri
   tachys reactive_graph virtual_list virtual_list_leptos getrandom itertools
   thiserror futures objc2 cocoa`
    .split(/\s+/)
    .filter((name) => name.length > 0),
);

const DECL = /\b(?:fn|struct|enum|trait|const|static|type|mod|union)\s+([A-Za-z_][A-Za-z0-9_]*)/g;
const FIELD = /\b([A-Za-z_][A-Za-z0-9_]*)\s*:/g;
const USE_LINE = /^\s*(?:pub(?:\([^)]*\))?\s+)?use\s[^;]*;/gm;
const GLOB_USE = /pub\s+use\s[^;]*::\*;/;
const MODULE_PATH = /\b(crate|super|self|[a-z][a-z0-9_]*)((?:::)[A-Za-z_][A-Za-z0-9_]*)+/g;
const FILE_PATH = /[\w.\/-]*[\w-]\/[\w.\/-]+\.(?:rs|ts|css|json|toml|md|html|mjs)/g;

/** The file that makes `base` a module, if any. */
function moduleFile(base: string): string | null {
  if (isFile(`${base}.rs`)) return `${base}.rs`;
  const mod = path.posix.join(base, "mod.rs");
  if (isFile(mod)) return mod;
  return null;
}

/** The crate root a file's `crate::` refers to. */
function crateRootOf(file: string): string {
  if (file.startsWith("src-tauri/")) return "src-tauri/src";
  if (file.startsWith("crates/")) return file.slice(0, file.indexOf("/src/")) + "/src";
  return "src";
}

/** The longest prefix of `segs` under `base` that is a module. */
function resolveModules(base: string, segs: string[]): { count: number; file: string | null } {
  let current = base;
  let count = 0;
  let file: string | null = null;
  for (let i = 0; i < segs.length; i++) {
    current = path.posix.join(current, segs[i]!);
    const found = moduleFile(current);
    if (!found) break;
    file = found;
    count = i + 1;
  }
  return { count, file };
}

/** The crate-root module file (`lib.rs` / `main.rs`), for facade re-exports. */
function rootModule(dir: string): string | null {
  for (const name of ["lib.rs", "main.rs", "mod.rs"]) {
    if (isFile(path.posix.join(dir, name))) return path.posix.join(dir, name);
  }
  return moduleFile(dir);
}

/** Whether `item` is something `file` declares, re-exports, or waves through. */
function declaresItem(file: string, item: string): boolean {
  const body = read(file);
  // A glob re-export forwards names this script cannot enumerate.
  if (GLOB_USE.test(body)) return true;
  for (const m of body.matchAll(DECL)) if (m[1] === item) return true;
  for (const m of body.matchAll(FIELD)) if (m[1] === item) return true;
  for (const m of body.matchAll(USE_LINE)) if (m[0].includes(item)) return true;
  return false;
}

const problems: string[] = [];
let checked = 0;

for (const file of RUST_FILES) {
  const crateRoot = crateRootOf(file);
  const fileDir = path.posix.dirname(file);
  // The module a file's own items live in: its directory, except for a `mod.rs`,
  // which IS its directory's module.
  const ownModule = path.posix.basename(file) === "mod.rs" ? path.posix.dirname(fileDir) : fileDir;
  // Where a comment's unprefixed module name (`page_host::block_row_id`) is
  // looked for: the crate root, then the file's own directory and its parents.
  // The file's DIRECTORY, not its module — prose in a `mod.rs` usually means a
  // sibling of the file it is written in.
  const ancestors = [crateRoot, fileDir];
  const top = crateRoot.split("/")[0]!;
  for (
    let up = path.posix.dirname(fileDir);
    up !== "." && up.startsWith(top);
    up = path.posix.dirname(up)
  ) {
    ancestors.push(up);
  }

  const lines = read(file).split("\n");
  lines.forEach((line, index) => {
    const at = line.indexOf("//");
    if (at < 0) return;
    const comment = line.slice(at);
    const lineNo = index + 1;

    // A URL's `://` would otherwise be read as a comment starting mid-string.
    const withoutUrl = comment.includes("://") ? comment.slice(0, comment.indexOf("://")) : comment;

    for (const m of withoutUrl.matchAll(MODULE_PATH)) {
      const whole = m[0];
      const first = m[1]!;
      let segs = whole.split("::").slice(1);
      if (SKIP_FIRST.has(first)) continue;

      let base: string;
      if (first === "crate") {
        base = crateRoot;
      } else if (first === "self") {
        base = ownModule;
      } else if (first === "super") {
        const all = whole.split("::");
        let supers = 0;
        while (supers < all.length && all[supers] === "super") supers++;
        segs = all.slice(supers);
        base = ownModule;
        for (let i = 1; i < supers; i++) base = path.posix.dirname(base);
      } else if (CRATE_ROOTS.has(first)) {
        base = CRATE_ROOTS.get(first)!;
      } else {
        // Prose that names a module without its crate: try the crate root and
        // the file's own neighbourhood. If nothing there has that name, it is
        // somebody else's crate and none of this script's business.
        const known = ancestors.find((dir) => moduleFile(path.posix.join(dir, first)));
        if (!known) continue;
        base = known;
        segs = [first, ...segs];
      }

      checked++;
      let { count, file: modFile } = resolveModules(base, segs);
      if (count === 0) {
        // Nothing below the root is a module, but the root itself may re-export
        // the name (a crate's facade `pub use`).
        const rootFile = rootModule(base);
        if (!rootFile) {
          problems.push(`${file}:${lineNo}: no module at \`${whole}\` (root ${base})`);
          continue;
        }
        modFile = rootFile;
      }
      if (!modFile) continue;
      if (count === segs.length) continue;
      const item = segs[count]!;
      if (!declaresItem(modFile, item)) {
        problems.push(
          `${file}:${lineNo}: \`${item}\` is not declared or re-exported by ${modFile} (from \`${whole}\`)`,
        );
      }
    }

    for (const m of comment.matchAll(FILE_PATH)) {
      const token = m[0].replace(/^[./]+/, "");
      // An elided path ("readest/.../traffic_light.rs") is prose, not a claim.
      if (token.includes("...")) continue;
      // Comments name a file by whatever tail is unambiguous, so match on the
      // end of a real path rather than guessing its prefix.
      const exists = ALL_FILES.some((candidate) => candidate === token || candidate.endsWith(`/${token}`));
      if (!exists) problems.push(`${file}:${lineNo}: no such file: \`${token}\``);
    }
  });
}

// ---------------------------------------------------------------------------
// Stylesheet comments: the same claim, in files the Rust pass never reads.
//
// A stylesheet names the module that writes its tokens ("painted by
// `appearance::reflowable`", "see src/zoom"), and those references are
// directories, or extension-less module paths, as often as they are files —
// which is why they need their own pattern: `FILE_PATH` above only matches a
// token that ends in a known extension. Six of them had rotted before this
// pass existed, all naming modules that a rename had moved.
// ---------------------------------------------------------------------------

/** Every directory in the tree, so an extension-less module path resolves. */
const DIRS = new Set<string>();
for (const file of ALL_FILES) {
  for (let dir = path.posix.dirname(file); dir !== "."; dir = path.posix.dirname(dir)) DIRS.add(dir);
}

const FILE_SET = new Set(ALL_FILES);
const CSS_FILES = ALL_FILES.filter((file) => file.endsWith(".css") && file.startsWith("styles/"));
/** A path token that starts at one of our roots. The extension is optional. */
/** Roots a stylesheet would name. `tests` and `scripts` are left out on
 *  purpose: prose uses a slash for "or" ("a hook for tests/overrides"), and no
 *  stylesheet has ever pointed at either tree. */
const CSS_PATH = /\b(?:src|crates|styles|public|src-tauri)\/[A-Za-z0-9_./-]+/g;
const SOURCE_EXTS = [".rs", ".ts", ".css", ".js", ".mjs", ".json", ".toml", ".html"];

/** Whether a token names a directory, a file, or a file whose extension the
 *  prose left off. */
function sourcePathResolves(token: string): boolean {
  if (DIRS.has(token) || FILE_SET.has(token)) return true;
  return SOURCE_EXTS.some((ext) => FILE_SET.has(token + ext));
}

let cssChecked = 0;

for (const file of CSS_FILES) {
  const text = read(file);
  for (const comment of text.matchAll(/\/\*[\s\S]*?\*\//g)) {
    // A URL's `://` would otherwise be read as a path with a very odd root.
    const raw = comment[0];
    const body = raw.includes("://") ? raw.slice(0, raw.indexOf("://")) : raw;
    for (const m of body.matchAll(CSS_PATH)) {
      // The TOKEN's line, not the comment's: a stylesheet's header block can
      // span fifteen lines, and a report that says line 1 sends the reader to
      // the top of the file to hunt for it.
      const lineNo = text.slice(0, comment.index + m.index).split("\n").length;
      // Prose punctuation and a trailing slash are not part of the path.
      const token = m[0].replace(/[.,;:)]+$/, "").replace(/\/+$/, "");
      if (!token.includes("/")) continue;
      cssChecked++;
      if (!sourcePathResolves(token)) problems.push(`${file}:${lineNo}: no such path: \`${token}\``);
    }
  }
}

// ---------------------------------------------------------------------------
// The two prose documents: the NAMES they put in backticks.
//
// A path is one kind of claim and a component name is another, and the second
// is what a rename leaves behind most often: `PageList` and `SinglePageView`
// outlived the components they named by several refactors, in prose no compiler
// reads, and the README documented two engine methods that do not exist.
//
// Only README.md and ARCHITECTURE.md are checked. A Rust doc comment names
// external types constantly (`Closure`, `TreeWalker`, `NSWindow`), and an
// allowlist long enough to cover those is one nobody maintains; these two
// documents describe this app, so a capitalised name in them is ours until
// proven otherwise.
// ---------------------------------------------------------------------------

/** Capitalised names the documents use that are not declarations anywhere in
 *  the workspace: keyboard keys a shortcut table has to spell, and the platform
 *  types the app talks to but does not define. Add to this only for one of
 *  those, never for something the tree should be declaring. */
const PROSE_NAMES = new Set(["Shift", "Space", "Escape", "Range"]);

const DECLARED = new Set<string>();
const RS_ITEM = /\b(?:fn|struct|enum|trait|type|union|mod)\s+([A-Z][A-Za-z0-9_]*)/g;
const RS_ENUM_BODY = /\benum\s+\w+[^{]*\{([^{}]*)\}/g;
const RS_VARIANT = /^\s*([A-Z][A-Za-z0-9_]*)/gm;
const TS_ITEM = /\b(?:class|function|const|interface|type)\s+([A-Z][A-Za-z0-9_]*)/g;

for (const file of ALL_FILES) {
  if (!/\.(rs|ts)$/.test(file) || file.startsWith("public/vendor/")) continue;
  const text = read(file);
  for (const m of text.matchAll(RS_ITEM)) DECLARED.add(m[1]!);
  for (const m of text.matchAll(TS_ITEM)) DECLARED.add(m[1]!);
  for (const m of text.matchAll(RS_ENUM_BODY)) {
    for (const variant of m[1]!.matchAll(RS_VARIANT)) DECLARED.add(variant[1]!);
  }
}

const BACKTICK_NAME = /`([A-Z][A-Za-z0-9]{3,})`/g;
let namesChecked = 0;

for (const file of ["README.md", "ARCHITECTURE.md"].filter(isFile)) {
  read(file).split("\n").forEach((line, index) => {
    for (const m of line.matchAll(BACKTICK_NAME)) {
      const name = m[1]!;
      namesChecked++;
      if (DECLARED.has(name) || PROSE_NAMES.has(name)) continue;
      problems.push(`${file}:${index + 1}: nothing in the workspace is named \`${name}\``);
    }
  });
}

// ---------------------------------------------------------------------------
// The documented engine surface.
//
// `window.PDFReader` is written down three times: the TypeScript type, the Rust
// bridge (which `crates/pdf-engine/tests/engine_contract.rs` holds against the
// built facade), and the README's "Engine API" table. Only the third was
// unchecked, and it had drifted — it advertised `buildSearchIndex` and `search`
// for a search index that lives in Rust, and omitted the appearance and paper
// methods entirely.
// ---------------------------------------------------------------------------

/** The member names of the facade's type, as declared. */
function apiMembers(): Set<string> {
  const table = "public/engine/types.ts";
  const body = /export type PDFReaderApi = \{([\s\S]*?)\n\};/.exec(read(table))?.[1];
  if (!body) throw new Error(`${table}: no PDFReaderApi type`);
  const out = new Set<string>();
  for (const m of body.matchAll(/^\s*(\w+)\s*:/gm)) out.add(m[1]!);
  return out;
}

const readme = isFile("README.md") ? read("README.md") : "";
const apiAt = readme.indexOf("### Engine API");
if (apiAt >= 0) {
  const nextHeading = readme.indexOf("\n## ", apiAt);
  const section = readme.slice(apiAt, nextHeading === -1 ? readme.length : nextHeading);
  const firstLine = readme.slice(0, apiAt).split("\n").length;
  const members = apiMembers();
  section.split("\n").forEach((line, index) => {
    // The table's first column, and only that: a bare lowerCamel token there is
    // a method name, while the prose around the table talks about envelope
    // fields (`ok`, `error`) that are not methods and must not be checked as
    // though they were.
    if (!line.startsWith("|")) return;
    const firstCell = line.split("|")[1] ?? "";
    for (const m of firstCell.matchAll(/`([a-z][A-Za-z0-9]+)`/g)) {
      namesChecked++;
      if (members.has(m[1]!)) continue;
      problems.push(`README.md:${firstLine + index}: \`${m[1]}\` is not a window.PDFReader method`);
    }
  });
}

// ---------------------------------------------------------------------------
// The two prose documents: the PATHS they put in backticks.
//
// ARCHITECTURE.md describes the tree by module path — `anchor::stroke_resolver`,
// `components::formats::reflow::stream` — and README.md points at files. Neither
// document is compiled, and a module path in prose carries no crate prefix to
// anchor it, so the pass above cannot read them: `anchor::x` in a Rust file means
// "a neighbour of mine", and in a document it means "wherever `anchor` lives".
//
// Resolution therefore starts from the NAME: every module in the tree is indexed
// by the name a document would call it, and a path is tried against each module
// that could be its first segment (plus the crate root, when the name is a
// crate). `block_view` is the drift this catches — the renderer dispatch was
// renamed `block_render`, and both documents kept sending readers to a module
// that did not exist.
// ---------------------------------------------------------------------------

/** Every Rust module in the tree, by the name a document would call it. */
const MODULES_BY_NAME = new Map<string, string[]>();
for (const file of RUST_FILES) {
  const base = file.slice(0, -3); // drop `.rs`
  const name = path.posix.basename(file) === "mod.rs" ? path.posix.dirname(base) : base;
  const key = path.posix.basename(name);
  const list = MODULES_BY_NAME.get(key);
  if (list) list.push(name);
  else MODULES_BY_NAME.set(key, [name]);
}

/**
 * A document's module path, resolved against every base its first segment could
 * mean. `anchor` is both `crates/virtual-list/src/anchor.rs` and
 * `src/components/ai/anchor/`, so an ambiguous name is not an error: the path is
 * good if ANY reading of it holds, and the near-miss is only reported when none
 * does. `prefix` is how many of the caller's segments the base itself consumed
 * (one, for a crate name).
 */
function resolveDocModules(
  segs: string[],
): { ok: boolean; near: string | null } {
  const first = segs[0]!;
  const attempts: Array<[string, string[], number]> = [];
  const crateRoot = CRATE_ROOTS.get(first);
  if (crateRoot) attempts.push([crateRoot, segs.slice(1), 1]);
  for (const base of MODULES_BY_NAME.get(first) ?? []) {
    attempts.push([path.posix.dirname(base), segs, 0]);
  }

  // The near-miss from the reading that got FURTHEST: `anchor` is two modules in
  // this tree, and reporting the one that resolved nothing would send whoever
  // fixes it to the wrong file.
  let near: string[] = [];
  let best = -1;
  for (const [base, rest, prefix] of attempts) {
    if (rest.length === 0) continue;
    const { count, file } = resolveModules(base, rest);
    if (count === 0) {
      // No module below the base, but the base's own facade may re-export the
      // name — which is how a crate's `lib.rs` answers for its whole surface.
      const rootFile = rootModule(base);
      if (rootFile && rest.length === 1 && declaresItem(rootFile, rest[0]!)) {
        return { ok: true, near: null };
      }
      continue;
    }
    const consumed = prefix + count;
    if (consumed >= segs.length) return { ok: true, near: null };
    const item = segs[consumed]!;
    if (file && declaresItem(file, item)) return { ok: true, near: null };
    // Two modules can share a name (`anchor` is one here), so an invented tail
    // is blamed on every reading that got this far, rather than on whichever
    // happened to be indexed first.
    if (consumed > best) {
      best = consumed;
      near = [];
    }
    near.push(file ?? base);
  }
  return {
    ok: false,
    near: near.length === 0 ? null : `\`${segs[best]!}\` is not declared or re-exported by ${near.join(" or ")}`,
  };
}

/** Roots a document would name. `target/` and `dist/` are build output. */
const DOC_PATH = /\b(?:src|crates|styles|public|scripts|tests|release-notes|src-tauri)\/[A-Za-z0-9_./-]*/g;

let docModules = 0;
let docPaths = 0;

for (const file of ["README.md", "ARCHITECTURE.md"].filter(isFile)) {
  read(file).split("\n").forEach((line, index) => {
    const lineNo = index + 1;

    for (const backticked of line.matchAll(/`([^`]*)`/g)) {
      for (const m of backticked[1]!.matchAll(MODULE_PATH)) {
        const whole = m[0];
        const segs = whole.split("::");
        if (SKIP_FIRST.has(segs[0]!)) continue;
        docModules++;
        const resolved = resolveDocModules(segs);
        if (resolved.ok) continue;
        problems.push(
          `${file}:${lineNo}: ${resolved.near ?? "no module there"} (from \`${whole}\`)`,
        );
      }
    }

    for (const m of line.matchAll(DOC_PATH)) {
      // Prose punctuation and a trailing slash are not part of the path.
      const token = m[0].replace(/[.,;:)]+$/, "").replace(/\/+$/, "");
      if (!token.includes("/")) continue;
      docPaths++;
      if (!sourcePathResolves(token)) problems.push(`${file}:${lineNo}: no such path: \`${token}\``);
    }
  });
}

if (problems.length > 0) {
  console.error(`::error::${problems.length} comment path(s) do not resolve:`);
  for (const problem of problems) console.error(`  ${problem}`);
  console.error("");
  console.error(
    "  A comment that names a module is a claim about the tree; rename the path with the code.",
  );
  process.exit(1);
}

console.log(
  `doc paths resolve: ${checked} module paths across ${RUST_FILES.length} Rust files, ` +
    `${cssChecked} paths across ${CSS_FILES.length} stylesheets, ` +
    `${namesChecked} names, ${docModules} module paths and ${docPaths} file paths ` +
    `in the two documents`,
);
