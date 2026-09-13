fn filter_source_load(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(source_texture, vec2<i32>(i32(x), i32(y)), 0));
}

fn filter_aux_load(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(textureLoad(aux_texture, vec2<i32>(i32(x), i32(y)), 0));
}

// Root fix: normalized sampler coordinates round differently for different pooled
// texture capacities. Interpolate in logical texel space so allocation history
// cannot change a filter's pixels. Clamp both taps to the logical image, not padding.
fn filter_linear_channel(a: f32, b: f32, c: f32, d: f32, phase: vec2<f32>) -> f32 {
    let top = fma(b - a, phase.x, a);
    let bottom = fma(d - c, phase.x, c);
    return fma(bottom - top, phase.y, top);
}

fn filter_sample_premul(image: texture_2d<f32>, x: f32, y: f32) -> vec4<f32> {
    let last = vec2<i32>(i32(config.width) - 1, i32(config.height) - 1);
    let position = clamp(vec2<f32>(x, y), vec2<f32>(0.0), vec2<f32>(last));
    let first = vec2<i32>(floor(position));
    let next = min(first + vec2<i32>(1), last);
    let phase = position - vec2<f32>(first);
    let a = textureLoad(image, first, 0);
    let b = textureLoad(image, vec2<i32>(next.x, first.y), 0);
    let c = textureLoad(image, vec2<i32>(first.x, next.y), 0);
    let d = textureLoad(image, next, 0);
    return vec4<f32>(
        filter_linear_channel(a.r, b.r, c.r, d.r, phase),
        filter_linear_channel(a.g, b.g, c.g, d.g, phase),
        filter_linear_channel(a.b, b.b, c.b, d.b, phase),
        filter_linear_channel(a.a, b.a, c.a, d.a, phase),
    );
}

fn filter_source_sample_premul(x: f32, y: f32) -> vec4<f32> {
    return filter_sample_premul(source_texture, x, y);
}

fn filter_aux_sample_premul(x: f32, y: f32) -> vec4<f32> {
    return filter_sample_premul(aux_texture, x, y);
}

fn filter_target_store(x: u32, y: u32, pixel: u32) {
    textureStore(target_texture, vec2<i32>(i32(x), i32(y)), rgba8_to_unorm(pixel));
}
