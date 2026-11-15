// One tinted quad per page or fill. `LINEAR` and `noise.wgsl` are
// prepended; `LINEAR` is true when the target encodes sRGB itself.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
    @location(2) at: vec2<f32>,
    @location(3) line: f32,
    @location(4) shape: vec3<f32>,
    @location(5) round: vec4<f32>,
};

// `rect`: clip corners; `pattern`: in units; `shape`, `round`: the mask.
@vertex
fn vs(
    @builtin(vertex_index) i: u32,
    @location(0) rect: vec4<f32>,
    @location(1) tint: vec4<f32>,
    @location(2) pattern: vec4<f32>,
    @location(3) line: f32,
    @location(4) shape: vec3<f32>,
    @location(5) round: vec4<f32>,
) -> VsOut {
    let corner = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    var out: VsOut;
    out.pos = vec4<f32>(mix(rect.xy, rect.zw, corner), 0.0, 1.0);
    out.uv = corner;
    out.tint = tint;
    out.at = mix(pattern.xy, pattern.zw, corner);
    out.line = line;
    out.shape = shape;
    out.round = round;
    return out;
}

@group(0) @binding(0) var page: texture_2d<f32>;
@group(0) @binding(1) var filtered: sampler;

// A `PageTone`. `mode`: op count, blend, texture, grain blend.
struct Tone {
    rows: array<vec4<f32>, 21>,
    paper: vec4<f32>,
    mode: vec4<u32>,
    stroke: vec4<f32>,
    grain: vec4<f32>,
    // x: the texture's opacity; yzw: a book's ink.
    params: vec4<f32>,
};
@group(1) @binding(0) var<uniform> tone: Tone;

// CSS filter functions, clamping after each, in sRGB terms.
fn filter_chain(c: vec3<f32>) -> vec3<f32> {
    var out = c;
    for (var i = 0u; i < min(tone.mode.x, 7u); i++) {
        let r = tone.rows[i * 3u];
        let g = tone.rows[i * 3u + 1u];
        let b = tone.rows[i * 3u + 2u];
        let m = vec3<f32>(dot(r.xyz, out) + r.w, dot(g.xyz, out) + g.w, dot(b.xyz, out) + b.w);
        out = clamp(m, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return out;
}

fn soft_light(b: f32, s: f32) -> f32 {
    if (s <= 0.5) {
        return b - (1.0 - 2.0 * s) * b * (1.0 - b);
    }
    var d = sqrt(b);
    if (b <= 0.25) {
        d = ((16.0 * b - 12.0) * b + 4.0) * b;
    }
    return b + (2.0 * s - 1.0) * (d - b);
}

// W3C blend modes: 1 multiply, 2 screen, 3 soft light; 4 recolours.
fn blend(b: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    switch tone.mode.y {
        case 1u: { return b * s; }
        case 2u: { return b + s - b * s; }
        default: {
            return vec3<f32>(soft_light(b.r, s.r), soft_light(b.g, s.g), soft_light(b.b, s.b));
        }
    }
}

// Box-filtered cover of strokes `w` wide every `pitch`; `fw` is a pixel.
fn band(x: f32, w: f32, pitch: f32, fw: f32) -> f32 {
    let d = x - floor(x / pitch) * pitch;
    let lo = d - fw * 0.5;
    let hi = d + fw * 0.5;
    var cover = 0.0;
    for (var k = -1; k <= 1; k++) {
        let s = f32(k) * pitch;
        cover += max(0.0, min(hi, s + w) - max(lo, s));
    }
    return min(cover / max(fw, 1e-4), 1.0);
}

// A dot centred in each `pitch` cell, solid to `r0`, gone at `r1`.
fn spot(p: vec2<f32>, pitch: f32, r0: f32, r1: f32, fw: f32) -> f32 {
    let r = length(p - (floor(p / pitch) + 0.5) * pitch);
    return clamp((r1 + fw * 0.5 - r) / (r1 - r0 + fw), 0.0, 1.0);
}

// feTurbulence-like octaves, 0..1, tiling every 180 units, minified to grey.
fn turbulence(p: vec2<f32>, fw: f32, seed: u32) -> f32 {
    let q = p * 0.65;
    let n = value_noise(q, 117, seed) + 0.5 * value_noise(q * 2.0, 234, seed + 7u);
    let g = clamp((n + 1.0) * 0.5, 0.0, 1.0);
    return mix(g, 0.5, clamp(fw * 0.65 - 0.5, 0.0, 1.0));
}

// The grain: noise blended into its tint, as `Pattern::grain_layer`.
fn grain_layer(g: f32, n: f32) -> vec4<f32> {
    let tint = tone.grain.rgb;
    let ab = tone.grain.a;
    let ai = 0.055 * n;
    var mixed = tint * g;
    if (tone.mode.w == 2u) {
        mixed = tint + vec3<f32>(g) - tint * g;
    }
    let a = ai + ab * (1.0 - ai);
    if (a <= 0.0) {
        return vec4<f32>(0.0);
    }
    let src = (1.0 - ab) * vec3<f32>(g) + ab * mixed;
    return vec4<f32>((ai * src + ab * (1.0 - ai) * tint) / a, a);
}

// Texture modes: 1 paper, 2 lined, 3 grid, 4 dotted, 5 cross.
fn pattern_layer(p: vec2<f32>, fw: vec2<f32>, line: f32) -> vec4<f32> {
    let ink = tone.stroke;
    let f = max(fw.x, fw.y);
    switch tone.mode.z {
        case 1u: {
            return grain_layer(turbulence(p, f, 1u), turbulence(p, f, 2u));
        }
        case 2u: {
            return vec4<f32>(ink.rgb, ink.a * band(p.y, line, 26.0, fw.y));
        }
        case 3u: {
            let h = ink.a * band(p.y, line, 26.0, fw.y);
            let v = ink.a * band(p.x, line, 26.0, fw.x);
            return vec4<f32>(ink.rgb, 1.0 - (1.0 - h) * (1.0 - v));
        }
        case 4u: {
            return vec4<f32>(ink.rgb, ink.a * spot(p, 8.0, 1.0, 1.5, f));
        }
        case 5u: {
            let h = ink.a * band(p.y, line, 26.0, fw.y);
            let v = ink.a * band(p.x, line, 26.0, fw.x);
            let d = ink.a * spot(p, 26.0, 1.5, 2.0, f);
            return vec4<f32>(ink.rgb, 1.0 - (1.0 - h) * (1.0 - v) * (1.0 - d));
        }
        default: {
            return vec4<f32>(0.0);
        }
    }
}

// How far a colour's grey may move toward the middle, making room for hue.
const HUE_SHIFT: f32 = 0.1;
// Themes with less contrast than this, as dim, mute hues to match.
const FULL_HUE: f32 = 0.5;

fn least(v: vec3<f32>) -> f32 {
    return min(min(v.x, v.y), v.z);
}

// A book's greys, black to its ink and white to its paper; colours keep
// their hue around that grey, as much of it as fits.
fn recolour(s: vec3<f32>) -> vec3<f32> {
    let luma = vec3<f32>(0.2126, 0.7152, 0.0722);
    let g = clamp(dot(s, luma), 0.0, 1.0);
    let span = abs(dot(tone.paper.rgb - tone.params.yzw, luma));
    let off = (s - vec3<f32>(g)) * clamp(span / FULL_HUE, 0.25, 1.0);
    var base = mix(tone.params.yzw, tone.paper.rgb, g);
    let lo = -least(off + base);
    let hi = least(vec3<f32>(1.0) - off - base);
    base += vec3<f32>(clamp(min(max(0.0, lo), hi), -HUE_SHIFT, HUE_SHIFT));
    let tiny = vec3<f32>(1e-4);
    let up = select(vec3<f32>(1.0), (vec3<f32>(1.0) - base) / max(off, tiny), off > tiny);
    let down = select(vec3<f32>(1.0), base / max(-off, tiny), off < -tiny);
    let k = clamp(min(least(up), least(down)), 0.0, 1.0);
    return clamp(base + off * k, vec3<f32>(0.0), vec3<f32>(1.0));
}

// The page filtered, blended over opaque paper, then textured.
fn themed(c: vec4<f32>, at: vec2<f32>, fw: vec2<f32>, line: f32) -> vec4<f32> {
    if (tone.mode.y == 0u) {
        return c;
    }
    let paper = tone.paper.rgb;
    let raw = c.rgb / max(c.a, 1e-5);
    var s = recolour(raw);
    if (tone.mode.y != 4u) {
        s = blend(paper, filter_chain(raw));
    }
    let page = mix(paper, s, c.a);
    let layer = pattern_layer(at, fw, line);
    let a = layer.a * tone.params.x;
    return vec4<f32>(page * (1.0 - a) + layer.rgb * a, 1.0);
}

// Rasters hold gamma-encoded, premultiplied colour.
fn to_target(c: vec4<f32>) -> vec4<f32> {
    if (!LINEAR || c.a <= 0.0) {
        return c;
    }
    let rgb = c.rgb / c.a;
    let low = rgb / 12.92;
    let high = pow((rgb + 0.055) / 1.055, vec3<f32>(2.4));
    return vec4<f32>(select(high, low, rgb <= vec3<f32>(0.04045)) * c.a, c.a);
}

// Cover by the rounded `round`; branch-free keeps derivatives uniform.
fn cover(uv: vec2<f32>, shape: vec3<f32>, round: vec4<f32>) -> f32 {
    let r = shape.z;
    let half = (round.zw - round.xy) * 0.5;
    let p = abs(uv * shape.xy - (round.xy + half)) - (half - r);
    let d = length(max(p, vec2<f32>(0.0))) + min(max(p.x, p.y), 0.0) - r;
    let c = clamp(0.5 - d / max(fwidth(d), 1e-4), 0.0, 1.0);
    return select(1.0, c, r > 0.0);
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let fw = fwidth(in.at);
    let c = themed(textureSample(page, filtered, in.uv), in.at, fw, in.line) * in.tint;
    return to_target(c) * cover(in.uv, in.shape, in.round);
}

// For the shrunk glass strip: a 4x4 average per pixel
// stops scrolled text shimmering.
@fragment
fn fs_strip(in: VsOut) -> @location(0) vec4<f32> {
    let step = fwidth(in.uv) / 4.0;
    let fw = fwidth(in.at);
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < 4; y++) {
        for (var x = 0; x < 4; x++) {
            let at = in.uv + (vec2<f32>(f32(x), f32(y)) - 1.5) * step;
            sum += textureSampleLevel(page, filtered, at, 0.0);
        }
    }
    let c = themed(sum / 16.0, in.at, fw, in.line) * in.tint;
    return to_target(c) * cover(in.uv, in.shape, in.round);
}
