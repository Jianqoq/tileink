@group(0) @binding(1) var target_texture: texture_storage_2d<rgba8unorm, write>;

fn target_load(_x: u32, _y: u32) -> u32 {
    return config.clear_color;
}

fn target_store(x: u32, y: u32, pixel: u32) {
    textureStore(target_texture, vec2<i32>(i32(x), i32(y)), rgba8_to_unorm(pixel));
}
