// The disposable host borrows the Shell's coordinator.
import { WindowRasterLane } from "./reader/raster-coordinator";

const params = new URLSearchParams(window.location.search);
if (params.get("hosted") === "1") {
  const generation = params.get("g");
  const nonce = params.get("n");
  const lane = window.parent !== window ? window.parent.__mareaderRasterLane : undefined;
  if (!generation || !nonce || !lane) throw new Error("Reader host has no Shell raster coordinator");
  window.__mareaderRasterLane = lane;
  window.__mareaderRasterScope = `reader:${generation}:${nonce}`;
} else {
  // The standalone development entry is its own top-level host.
  window.__mareaderRasterLane ??= new WindowRasterLane();
  window.__mareaderRasterScope = "standalone";
}
