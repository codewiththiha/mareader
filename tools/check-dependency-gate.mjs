// The boundary from `cargo tree`: no crate reaches an engine or parser.

import { execFileSync } from "node:child_process";

/** Workspace crates carrying the boundary and the crates forbidden inside. */
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
  {
    crate: "reader-runtime",
    features: ["--no-default-features", "--features", "reflow"],
    forbid: ["pdf-engine"],
  },
  {
    // The persistent Shell cannot retain route code in its own WASM heap.
    crate: "mareader",
    forbid: [...READER_ONLY, "library-runtime"],
  },
  {
    crate: "reader-runtime",
    features: ["--no-default-features"],
    forbid: ["pdf-engine"],
  },
  {
    // The shelf knows the PDF as format only; no reader code in its graphs.
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

/** One crate's graph: the crate itself then every resolved dependency line. */
function tree(crate, features = []) {
  let out;
  try {
    out = execFileSync(
      "cargo",
      ["tree", "-p", crate, "--locked", "--edges", "normal,build", "--prefix", "none", ...features],
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
    names = tree(rule.crate, rule.features);
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
