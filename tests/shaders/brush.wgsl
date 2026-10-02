struct RequestConfig { count:u32, pad0:u32, pad1:u32, pad2:u32 };
@group(0) @binding(11) var<uniform> request_config:RequestConfig;
@group(0) @binding(9) var<storage,read> requests:array<u32>;
@group(0) @binding(10) var<storage,read_write> output:array<u32>;
@compute @workgroup_size(FINE_WORKGROUP_SIZE)
fn brush_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if(id.x>=request_config.count) {return;}
    let offset=id.x*4u;
    output[id.x]=sample_brush(requests[offset],bitcast<f32>(requests[offset+1u]),bitcast<f32>(requests[offset+2u]));
}
