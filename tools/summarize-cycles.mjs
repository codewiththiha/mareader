// The split-close-cycles table for the Deep CI step summary, from the
// `RESULT {...}` lines tools/measure-split-cycles.mjs printed to cycles.log.
import { appendFileSync, existsSync, readFileSync } from "node:fs";

const file = process.argv[2] ?? "cycles.log";
if (!existsSync(file)) process.exit(0);
const out = [
  "| engine | PSS one pane | after warm-up | cycles | after last | +30 s idle | after pressure | min–max (steady) | PSS slope MB/cycle | wasm memory MB first→last | wasm slope | DOM elements first→last | canvases first→last | sessions / workers live |",
  "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|",
];
for (const line of readFileSync(file, "utf8").split("\n").filter((l) => l.startsWith("RESULT "))) {
  const r = JSON.parse(line.slice(7));
  const m = r.summary;
  out.push(
    `| ${r.engine} | ${m.pssOnePane} | ${m.pssAfterWarmup} | ${r.cycles} | ${m.pssAfterLast} | ${m.pssAfterIdle30s ?? "–"} | ${m.pssAfterPressure ?? "–"} | ${m.pssMin}–${m.pssMax} | ${m.pssSlopeMBPerCycle} | ${m.wasmMemoryFirstLastMB.join("→")} | ${m.wasmMemorySlopeMBPerCycle} | ${m.elementsFirstLast.join("→")} | ${m.canvasesFirstLast.join("→")} | ${m.sessionsLiveLast} / ${m.workersLiveLast} |`,
  );
}
console.log(out.join("\n"));
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n${out.join("\n")}\n`);
