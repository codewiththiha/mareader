// Pure host/engine seam: no document, worker or engine implementation types.
export const RASTER_LANE_KEY = "__mareaderRasterLane";
export const WINDOW_RASTER_LIMIT = 2;

export interface RasterLaneSnapshot {
  limit: number;
  active: number;
  queued: number;
  owners: number;
  peakActive: number;
}

export interface RasterLane {
  request(owner: string, id: string, wake: () => void): boolean;
  cancel(owner: string, id: string): void;
  release(owner: string, id: string): void;
  retire(owner: string): void;
  retireScope(scope: string): void;
  snapshot(): RasterLaneSnapshot;
}

export interface RasterPermit {
  release(): void;
}

declare global {
  interface Window {
    __mareaderRasterLane?: RasterLane;
    __mareaderRasterScope?: string;
  }
}
