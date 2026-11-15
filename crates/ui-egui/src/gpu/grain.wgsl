// Film grain over the window or a preset card, as `FilmGrain::over`.
// `LINEAR` and `noise.wgsl` are prepended.

struct Grain {
    // The crawl's offset in points, the layer's opacity, pixels per point.
    offset: vec2<f32>,
    opacity: f32,
    ppp: f32,
    // The area grained, in points: min and max corners, and corner radius.
    area: vec4<f32>,
    radius: f32,
    // Tiles per tile: a card's grain is four times finer.
    scale: f32,
};
@group(0) @binding(0) var<uniform> grain: Grain;

// One triangle covering the target.
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// The tile's colour and alpha at point `p`, as `grain::layer`.
fn layer(p: vec2<f32>) -> vec4<f32> {
    let q = p * 0.9;
    var out = vec4<f32>(0.0);
    for (var c = 0u; c < 4u; c++) {
        let seed = 11u + c * 31u;
        let n = value_noise(q, 144, seed) + 0.5 * value_noise(q * 2.0, 288, seed + 7u);
        // NOISE_SPREAD in grain.rs: feTurbulence's deviation.
        out[c] = clamp(0.5 + n * 0.25, 0.0, 1.0);
    }
    return out;
}

// How much of the pixel at `p` lies inside the rounded area.
fn cover(p: vec2<f32>) -> f32 {
    let centre = (grain.area.xy + grain.area.zw) * 0.5;
    let half = (grain.area.zw - grain.area.xy) * 0.5 - grain.radius;
    let q = abs(p - centre) - half;
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - grain.radius;
    return clamp(0.5 - d * grain.ppp, 0.0, 1.0);
}

// The tile at `pos`; its alpha already holds opacity and cover.
fn at(pos: vec4<f32>) -> vec4<f32> {
    let p = pos.xy / grain.ppp;
    let n = layer((p - grain.offset) * grain.scale);
    return vec4<f32>(n.rgb, n.a * grain.opacity * cover(p));
}

// Exact for multiply, whose factors gamma maps alike; close for screen.
fn encode(c: vec3<f32>) -> vec3<f32> {
    if (!LINEAR) {
        return c;
    }
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

// Blended as source times destination.
@fragment
fn fs_multiply(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let n = at(pos);
    let a = n.a;
    return vec4<f32>(encode(vec3<f32>(1.0 - a) + a * n.rgb), 1.0);
}

// Blended as source plus destination times one minus source.
@fragment
fn fs_screen(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let n = at(pos);
    return vec4<f32>(encode(n.rgb * n.a), 1.0);
}
