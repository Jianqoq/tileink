@group(1) @binding(30) var texture_table:binding_array<texture_2d<f32>,NATIVE_TEXTURE_TABLE_CAPACITY>;
@group(0) @binding(0) var<storage,read> requests:array<u32>;
@group(0) @binding(1) var output:texture_storage_2d<rgba8unorm,write>;
@compute @workgroup_size(FINE_WORKGROUP_SIZE)
fn texture_table_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if(id.x>=arrayLength(&requests)/3u){return;}
    let offset=id.x*3u;
    textureStore(output,vec2<i32>(i32(id.x),0),textureLoad(texture_table[requests[offset]],vec2<i32>(i32(requests[offset+1u]),i32(requests[offset+2u])),0));
}
