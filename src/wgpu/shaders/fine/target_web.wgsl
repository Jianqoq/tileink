@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(54) var target_texture: texture_storage_2d<rgba8unorm, write>;

fn target_load_unorm(x: u32, y: u32) -> vec4<f32> {
    return textureLoad(source_texture, vec2<i32>(i32(x), i32(y)), 0);
}

fn target_load(x: u32, y: u32) -> u32 {
    return unorm_to_rgba8(target_load_unorm(x, y));
}

fn target_store_unorm(x: u32, y: u32, pixel: vec4<f32>) {
    textureStore(target_texture, vec2<i32>(i32(x), i32(y)), pixel);
}

fn target_store(x: u32, y: u32, pixel: u32) {
    target_store_unorm(x, y, rgba8_to_unorm(pixel));
}
