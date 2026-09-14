struct TextureConfig { width:u32, height:u32, layer:u32, pad1:u32 }
@group(0) @binding(0) var<uniform> config:TextureConfig;
@group(0) @binding(1) var source:texture_2d_array<f32>;
@group(0) @binding(2) var destination:texture_storage_2d<rgba8unorm, write>;
@compute @workgroup_size(FINE_WORKGROUP_SIZE, 1, 1)
fn texture_layer(@builtin(global_invocation_id) id:vec3<u32>) {
    if (id.x >= config.width || id.y >= config.height) { return; }
    textureStore(destination, vec2<i32>(id.xy), textureLoad(source, vec2<i32>(id.xy), i32(config.layer), 0));
}
