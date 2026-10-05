// The persistent Shell owns the only hosted full-page raster budget.
import { WindowRasterLane } from "./reader/raster-coordinator";

window.__mareaderRasterLane ??= new WindowRasterLane();
