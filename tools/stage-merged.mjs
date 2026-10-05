#!/usr/bin/env node
// Trunk's first Shell leg may run before runtime outputs exist on a clean
// checkout. Existing outputs are staged deterministically; IO failures are
// fatal, and the canonical builder requires the complete set after all legs.
import { mergeRuntimeArtifacts } from "./runtime-artifacts.mjs";

const staging = process.env.TRUNK_STAGING_DIR;
if (staging) {
  const copied = mergeRuntimeArtifacts(staging, { required: false });
  if (copied.length) console.log(`[stage-merged] distribution carries: ${copied.join(", ")}`);
}
