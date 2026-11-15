// Lattice noise shared by the page and grain shaders.
// `mareader_appearance::grain` mirrors it bit for bit.

fn hash(p: vec2<i32>, seed: u32) -> f32 {
    var h = (bitcast<u32>(p.x) * 0x8da6b343u) ^ (bitcast<u32>(p.y) * 0xd8163841u);
    h = (h ^ (seed * 0xcb1ab31fu)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h) / 4294967295.0;
}

// Smooth value noise, -1..1, repeating every `cells`.
fn value_noise(p: vec2<f32>, cells: i32, seed: u32) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let n = vec2<i32>(cells);
    let a = hash(((i % n) + n) % n, seed);
    let b = hash(((i + vec2<i32>(1, 0)) % n + n) % n, seed);
    let c = hash(((i + vec2<i32>(0, 1)) % n + n) % n, seed);
    let d = hash(((i + vec2<i32>(1, 1)) % n + n) % n, seed);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 2.0 - 1.0;
}
