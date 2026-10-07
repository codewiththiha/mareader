#!/usr/bin/env node
// Stage whatever outputs exist; the builder requires the full set later.
import { mergeRuntimeArtifacts } from "./runtime-artifacts.mjs";

const staging = process.env.TRUNK_STAGING_DIR;
if (staging) {
  const copied = mergeRuntimeArtifacts(staging, { required: false });
  if (copied.length) console.log(`[stage-merged] distribution carries: ${copied.join(", ")}`);
}
