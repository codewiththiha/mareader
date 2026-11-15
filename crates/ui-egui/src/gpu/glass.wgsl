// The glass: a separable Gaussian over the strip, then a composite.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle covering the target; uv runs 0..1 across it.
@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VsOut;
    out.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

// Blur: `v` one texel step, `a` sigma in texels.
// Composite: `v` uv scale, `a` saturation, `b` opacity, `o` uv start.
struct Params {
    v: vec2<f32>,
    a: f32,
    b: f32,
    o: vec2<f32>,
    pad: vec2<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var filtered: sampler;
@group(0) @binding(2) var<uniform> p: Params;

@fragment
fn fs_blur(in: VsOut) -> @location(0) vec4<f32> {
    let radius = i32(ceil(p.a * 3.0));
    let k = -0.5 / (p.a * p.a);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var i = -radius; i <= radius; i++) {
        let w = exp(f32(i * i) * k);
        sum += textureSampleLevel(src, filtered, in.uv + p.v * f32(i), 0.0) * w;
        weight += w;
    }
    return sum / weight;
}

// Rec. 709 luma, as CSS saturate() uses.
@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(src, filtered, p.o + in.uv * p.v, 0.0);
    let luma = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let rgb = clamp(mix(vec3<f32>(luma), c.rgb, p.a), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb, 1.0) * p.b;
}
