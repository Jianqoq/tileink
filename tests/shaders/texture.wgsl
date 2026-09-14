struct TextureConfig { width:u32, height:u32, pad0:u32, pad1:u32 };
@group(0) @binding(0) var<uniform> config:TextureConfig;
@group(0) @binding(1) var source:texture_2d<f32>;
@group(0) @binding(2) var destination:texture_storage_2d<rgba8unorm,write>;
@compute @workgroup_size(FINE_WORKGROUP_SIZE)
fn texture_flip(@builtin(global_invocation_id) id:vec3<u32>) {
    if (id.x>=config.width || id.y>=config.height) {return;}
    let pixel=textureLoad(source,vec2<i32>(i32(config.width-1u-id.x),i32(config.height-1u-id.y)),0);
    textureStore(destination,vec2<i32>(id.xy),pixel.bgra);
}
