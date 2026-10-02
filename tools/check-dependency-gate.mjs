// The compile-level runtime boundary, asserted from the dependency graph
// itself.
//
// The guide's separation rule is not a grep rule ("the library must not load
// engine scripts") — those catch imports, not graphs. The real requirement is
// that no runtime or shared crate can pull an engine, a paginator, a parser
// or a virtualizer it does not own, TRANSITIVELY: a `library-runtime` build
// that reaches `pdf-engine` through three middle crates is exactly the
// regression this gate exists to name. Only `cargo tree` answers that: it
// walks the resolved graph, repeats included, so an engine arriving through
// any chain still appears by name.
//
// One rule per (crate, forbidden set). The script shells out to
// `cargo tree -p <crate>` and fails if any forbidden crate appears anywhere
// in the output — direct or transitive. `--edges normal,build` on purpose:
// dev-dependencies (host-only test tooling) do not ship in the artifact and
// are not the boundary.
//
// A rule about a crate that does not exist yet fails the run, so a rename
// cannot silently retire the gate.
//
// Run it wherever cargo and node both live (the CI lint lane does). Like the
// runtime-artifacts check: plain node modules only, so no install step is
// needed in a lane that has not run `npm ci` yet.

import { execFileSync } from "node:child_process";

/** The workspace crates whose graphs carry the boundary. `forbid` names the
 *  crates that must NEVER appear in that graph, at any depth.
 */
/** The crates only a READER owns: the PDF engine and its geometry, the
 *  reflowable paginator and its parsers, the virtualizer and its Leptos
 *  binding, the markdown renderer. Neither the Shell nor the shelf may reach
 *  any of them — the library route has to be able to run, and rest, with no
 *  reader code resident, and a graph that pulls one of these compiles reader
 *  code into the library artifact whether or not anything calls it.
 */
const READER_ONLY = [
  "pdf-engine",
  "pdf-core",
  "reader-runtime",
  "reflow-core",
  "md-core",
  "txt-core",
  "virtual-list",
  "virtual-list-leptos",
  "leptos-md",
];

const RULES = [
  // The Shell (`mareader`) links the reader runtime on purpose: the
  // workspace host runs in the Shell's document. Documents never do — each
  // pane is a frame with its own artifact — so the engine is unreachable
  // there, and `tools/check-runtime-artifacts.mjs` proves it on the built
  // Shell (its glue imports nothing from `PDFReader`).
  {
    // The shelf knows the PDF as FORMAT metadata only (`Format::Pdf`); the
    // execution of one — engine and geometry both — belongs to the reader,
    // and the shelf's covers are baked by the Shell's bake page. The shelf
    // shares appearance and settings with the reader through persisted
    // storage (`storage`, `reader-core::settings`) and nothing else, so this
    // graph must carry no reader code at any depth.
    crate: "library-runtime",
    forbid: READER_ONLY,
  },
  {
    crate: "app-state",
    forbid: [
      "pdf-engine",
      "pdf-core",
      "virtual-list",
      "virtual-list-leptos",
      "reflow-core",
      "md-core",
      "txt-core",
      "ai-core",
      "leptos-md",
    ],
  },
  {
    crate: "runtime-contract",
    forbid: [
      "pdf-engine",
      "pdf-core",
      "leptos",
      "leptos-md",
      "virtual-list",
      "virtual-list-leptos",
      "ai-core",
    ],
  },
  {
    crate: "reader-core",
    forbid: ["pdf-engine", "pdf-core"],
  },
];

/** The dependency names of one crate's graph: the crate itself first, then
 *  every resolved dependency line `cargo tree` prints. Duplicates collapse. */
function tree(crate) {
  let out;
  try {
    out = execFileSync(
      "cargo",
      ["tree", "-p", crate, "--locked", "--edges", "normal,build", "--prefix", "none"],
      { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    );
  } catch (error) {
    const stderr = error.stderr ? String(error.stderr).trim() : String(error);
    throw new Error(`cargo tree -p ${crate} failed:\n${stderr}`);
  }
  const names = new Set();
  for (const line of out.split("\n")) {
    const name = line.trim().split(/\s+/)[0];
    if (name) names.add(name);
  }
  return names;
}

let failed = false;
for (const rule of RULES) {
  let names;
  try {
    names = tree(rule.crate);
  } catch (error) {
    console.error(error.message);
    failed = true;
    continue;
  }
  const hits = rule.forbid.filter((name) => names.has(name));
  if (hits.length > 0) {
    console.error(
      `Dependency gate: ${rule.crate} reaches ${hits.join(", ")} — ` +
        `a runtime/graph may not pull a crate it does not own, at any depth.`,
    );
    failed = true;
  }
}

if (failed) process.exit(1);
console.log(`Dependency gate: ${RULES.length} crate graphs clean.`);
