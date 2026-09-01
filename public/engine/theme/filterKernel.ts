// The CSS-filter pixel kernel: parse a filter string into a 3×3 matrix +
// offset, and apply it to RGBA pixels with per-channel LUTs (16.16 fixed
// point, no per-pixel multiply).
//
// This module is intentionally DOM-free: it is imported BOTH by the main
// thread (the no-worker fallback) and by bake.worker.ts (the same math, in a
// worker), so the two paths are byte-identical by construction and the
// fallback — which the Node smoke test exercises — IS the reference
// implementation.
//
// LUTs are memoized per filter string: a bake builds 9 × Int32Array(256) and
// rebuilding them per page per theme change was 9 allocations each time, even
// when the filter had not changed.

import type { FilterMatrix } from "../types";

const LU_TSCALE = 1 << 16;
// Add half a fixed-point unit before shifting so the final channel value is
// rounded to nearest instead of floored. The compositor applies the same
// pipeline with floating-point math; matching its rounding avoids a one-LSB
// seam between baked page pixels and the CSS backdrop.
const LU_ROUND = 1 << 15;

const lutCache = new Map<string, Int32Array[]>();

function filterTokenToMatrix(tok: string): FilterMatrix | null {
  const m = /^([a-z-]+)\(([^)]*)\)$/.exec(String(tok).trim());
  if (!m || !m[1] || !m[2]) return null;
  const name = m[1];
  const arg = parseFloat(m[2]);
  if (!Number.isFinite(arg)) return null;
  switch (name) {
    case "invert": {
      const k = 1 - 2 * arg;
      return { m: [k, 0, 0, 0, k, 0, 0, 0, k], o: [arg, arg, arg] };
    }
    case "brightness":
      return { m: [arg, 0, 0, 0, arg, 0, 0, 0, arg], o: [0, 0, 0] };
    case "contrast": {
      const off = 0.5 * (1 - arg);
      return { m: [arg, 0, 0, 0, arg, 0, 0, 0, arg], o: [off, off, off] };
    }
    case "saturate": {
      const t = 1 - arg;
      const a = 0.213 * t;
      const b = 0.715 * t;
      const c = 0.072 * t;
      return {
        m: [a + arg, b, c, a, b + arg, c, a, b, c + arg],
        o: [0, 0, 0],
      };
    }
    case "sepia": {
      const S = [0.393, 0.769, 0.189, 0.349, 0.686, 0.168, 0.272, 0.534, 0.131];
      const out: number[] = [];
      for (let i = 0; i < 9; i += 1) {
        const ident = i === 0 || i === 4 || i === 8 ? 1 : 0;
        out.push((1 - arg) * ident + arg * (S[i] ?? 0));
      }
      return { m: out, o: [0, 0, 0] };
    }
    case "hue-rotate": {
      const th = (arg * Math.PI) / 180;
      const c = Math.cos(th);
      const s = Math.sin(th);
      return {
        m: [
          0.213 + 0.787 * c - 0.213 * s,
          0.715 - 0.715 * c - 0.715 * s,
          0.072 - 0.072 * c + 0.928 * s,
          0.213 - 0.213 * c + 0.143 * s,
          0.715 + 0.285 * c + 0.140 * s,
          0.072 - 0.072 * c - 0.283 * s,
          0.213 - 0.213 * c - 0.787 * s,
          0.715 - 0.715 * c + 0.715 * s,
          0.072 + 0.928 * c + 0.072 * s,
        ],
        o: [0, 0, 0],
      };
    }
    default:
      return null;
  }
}

/** Compose a filter string into one 3×3 matrix + offset (row-major). */
export function composeFilter(filterString: string): FilterMatrix {
  let m = [1, 0, 0, 0, 1, 0, 0, 0, 1];
  let o = [0, 0, 0];
  for (const tok of String(filterString).split(/\s+/)) {
    if (!tok) continue;
    const op = filterTokenToMatrix(tok);
    if (!op) continue;
    const nm: number[] = [];
    const no: number[] = [];
    for (let r = 0; r < 3; r += 1) {
      nm[r * 3] =
        op.m[r * 3] * m[0] + op.m[r * 3 + 1] * m[3] + op.m[r * 3 + 2] * m[6];
      nm[r * 3 + 1] =
        op.m[r * 3] * m[1] + op.m[r * 3 + 1] * m[4] + op.m[r * 3 + 2] * m[7];
      nm[r * 3 + 2] =
        op.m[r * 3] * m[2] + op.m[r * 3 + 1] * m[5] + op.m[r * 3 + 2] * m[8];
      no[r] =
        op.m[r * 3] * o[0] + op.m[r * 3 + 1] * o[1] + op.m[r * 3 + 2] * o[2] + op.o[r];
    }
    m = nm;
    o = no;
  }
  return { m, o };
}

function lutsFor(m: number[], filterString: string): Int32Array[] {
  let luts = lutCache.get(filterString);
  if (!luts) {
    luts = new Array(9);
    for (let i = 0; i < 9; i += 1) {
      const coef = m[i] ?? 0;
      const lut = new Int32Array(256);
      for (let v = 0; v < 256; v += 1) lut[v] = Math.round(coef * v * LU_TSCALE);
      luts[i] = lut;
    }
    lutCache.set(filterString, luts);
  }
  return luts;
}

/** In-place RGB filter over `data` (RGBA, w×h). The identity matrix leaves
 *  the pixels untouched; a zero-size image is a no-op. */
export function applyFilterToData(
  data: Uint8ClampedArray,
  w: number,
  h: number,
  filterString: string,
): boolean {
  if (w <= 0 || h <= 0) return false;
  const { m, o } = composeFilter(filterString);
  const identity =
    m[0] === 1 && m[1] === 0 && m[2] === 0 &&
    m[3] === 0 && m[4] === 1 && m[5] === 0 &&
    m[6] === 0 && m[7] === 0 && m[8] === 1 &&
    o[0] === 0 && o[1] === 0 && o[2] === 0;
  if (identity) return false;

  const luts = lutsFor(m, filterString);
  const o0 = Math.round((o[0] ?? 0) * 255 * LU_TSCALE);
  const o1 = Math.round((o[1] ?? 0) * 255 * LU_TSCALE);
  const o2 = Math.round((o[2] ?? 0) * 255 * LU_TSCALE);
  const L0 = luts[0]!, L1 = luts[1]!, L2 = luts[2]!;
  const L3 = luts[3]!, L4 = luts[4]!, L5 = luts[5]!;
  const L6 = luts[6]!, L7 = luts[7]!, L8 = luts[8]!;
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i]!;
    const g = data[i + 1]!;
    const b = data[i + 2]!;
    data[i] = (L0[r] + L1[g] + L2[b] + o0 + LU_ROUND) >> 16;
    data[i + 1] = (L3[r] + L4[g] + L5[b] + o1 + LU_ROUND) >> 16;
    data[i + 2] = (L6[r] + L7[g] + L8[b] + o2 + LU_ROUND) >> 16;
  }
  return true;
}
