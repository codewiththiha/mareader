// Format-list sync check: registry, shell gate and tauri.conf.json must agree.

import { read } from "./repo.js";

/** One openable kind, as `reader_core::format::SUPPORTED` declares it. */
type Kind = { name: string; extensions: string[]; mimes: string[] };

const REGISTRY = "crates/reader-core/src/format.rs";
const SHELL_GATE = "src-tauri/src/lib.rs";
const BUNDLE_CONF = "src-tauri/tauri.conf.json";

// Parse the `SUPPORTED` table rather than import it; no JSON step needed.
function parseRegistry(): Kind[] {
  const text = read(REGISTRY);
  const table = /pub const SUPPORTED.+?=\s*&?\[([\s\S]*?)\n\];/.exec(text);
  if (!table || !table[1]) throw new Error(`${REGISTRY}: cannot find the SUPPORTED table`);

  const kinds: Kind[] = [];
  const row = /DocumentKind\s*\{([\s\S]*?)\}/g;
  for (let m = row.exec(table[1]); m; m = row.exec(table[1])) {
    const body = m[1]!;
    const name = /name\s*:\s*"([^"]+)"/.exec(body)?.[1];
    const extensions = list(body, "extensions");
    const mimes = list(body, "mimes");
    if (!name || extensions.length === 0) {
      throw new Error(`${REGISTRY}: a DocumentKind row is missing its name or extensions`);
    }
    kinds.push({ name, extensions, mimes });
  }
  if (kinds.length === 0) throw new Error(`${REGISTRY}: SUPPORTED parsed as empty`);
  return kinds;
}

/** The quoted items of one `field: &["a", "b"]` list inside a row body. */
function list(body: string, field: string): string[] {
  const m = new RegExp(`${field}\\s*:\\s*&\\[([^\\]]*)]`).exec(body);
  if (!m || !m[1]) return [];
  return [...m[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]!);
}

function parseShellGate(): string[] {
  const text = read(SHELL_GATE);
  const m = /const DOCUMENT_EXTENSIONS.+?=\s*&\[([^\]]*)]/.exec(text);
  if (!m || !m[1]) throw new Error(`${SHELL_GATE}: cannot find DOCUMENT_EXTENSIONS`);
  const exts = [...m[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]!);
  if (exts.length === 0) throw new Error(`${SHELL_GATE}: DOCUMENT_EXTENSIONS parsed as empty`);
  // The gate matches with `ends_with`, so its entries carry the dot.
  if (exts.some((ext) => !ext.startsWith("."))) {
    throw new Error(`${SHELL_GATE}: an extension is missing its leading dot`);
  }
  return exts;
}

// What the installer registers with the OS.
type Association = { ext?: string[]; mimeType?: string };

function parseAssociations(): Association[] {
  const conf = JSON.parse(read(BUNDLE_CONF)) as {
    bundle?: { fileAssociations?: Association[] };
  };
  const associations = conf.bundle?.fileAssociations;
  if (!associations || associations.length === 0) {
    throw new Error(`${BUNDLE_CONF}: cannot find bundle.fileAssociations`);
  }
  return associations;
}

const registry = parseRegistry();
const problems: string[] = [];

const registryExts = registry.flatMap((kind) => kind.extensions).sort();
const gateExts = parseShellGate().map((ext) => ext.slice(1)).sort();

if (registryExts.join(",") !== gateExts.join(",")) {
  problems.push(
    `${SHELL_GATE}: DOCUMENT_EXTENSIONS is ${gateExts.join(", ")} ` +
      `but ${REGISTRY} SUPPORTED is ${registryExts.join(", ")}`,
  );
}

const associations = parseAssociations();
const associatedExts = associations.flatMap((entry) => entry.ext ?? []).sort();
if (registryExts.join(",") !== associatedExts.join(",")) {
  problems.push(
    `${BUNDLE_CONF}: fileAssociations covers ${associatedExts.join(", ")} ` +
      `but ${REGISTRY} SUPPORTED is ${registryExts.join(", ")}`,
  );
}

// One association per kind, and each one naming that kind's extensions — the
// OS groups by association, so a kind split across two entries would show up
// as two document types in a "Open with" menu.
if (associations.length !== registry.length) {
  problems.push(
    `${BUNDLE_CONF}: ${associations.length} fileAssociations for ${registry.length} kinds in SUPPORTED`,
  );
}
for (const kind of registry) {
  const wanted = [...kind.extensions].sort().join(",");
  const entry = associations.find(
    (association) => [...(association.ext ?? [])].sort().join(",") === wanted,
  );
  if (!entry) {
    problems.push(
      `${BUNDLE_CONF}: no fileAssociation for ${kind.name} (${kind.extensions.join(", ")})`,
    );
    continue;
  }
  // The registry lists every MIME a drag may advertise the kind under; the
  // bundle wants the one the OS should show — which has to be one the format
  // actually answers to, or the association lies about what it opens.
  if (kind.mimes.length > 0 && entry.mimeType && !kind.mimes.includes(entry.mimeType)) {
    problems.push(
      `${BUNDLE_CONF}: ${kind.name} is registered as ${entry.mimeType}, ` +
        `which ${REGISTRY} does not list (${kind.mimes.join(", ")})`,
    );
  }
}

if (problems.length > 0) {
  console.error("::error::Supported-format lists disagree:");
  for (const problem of problems) console.error(`  ${problem}`);
  console.error("");
  console.error(
    `  ${REGISTRY} is the registry; the other two are derived from it.`,
  );
  process.exit(1);
}

console.log(
  `formats agree: ${registry.map((kind) => `${kind.name} (${kind.extensions.join("/")})`).join(", ")}`,
);
