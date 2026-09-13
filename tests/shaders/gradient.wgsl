struct RequestConfig { count:u32, pad0:u32, pad1:u32, pad2:u32 };
@group(0) @binding(11) var<uniform> request_config:RequestConfig;
// These explicit entries call the production brush functions without requiring
// texture resources that only pattern branches of sample_brush would access.
@group(0) @binding(9) var<storage,read> requests:array<u32>;
@group(0) @binding(10) var<storage,read_write> output:array<u32>;
@compute @workgroup_size(FINE_WORKGROUP_SIZE)
fn gradient_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if(id.x>=request_config.count) {return;}
    let offset=id.x*4u;
    let data_base=requests[offset];let x=bitcast<f32>(requests[offset+1u]);let y=bitcast<f32>(requests[offset+2u]);
    let kind=brush_word(data_base);let extend=brush_word(data_base+1u);
    let payload=data_base+brush_word(data_base+2u);let len=brush_word(data_base+3u);
    let base=data_base+GPU_BRUSH_U32_STRIDE;
    var color=brush_word(data_base+4u);
    if(kind==GPU_BRUSH_LINEAR) {color=sample_linear(x,y,base,extend,payload,len);}
    else if(kind==GPU_BRUSH_RADIAL) {color=sample_radial(x,y,base,extend,payload,len);}
    else if(kind==GPU_BRUSH_SWEEP) {color=sample_sweep(x,y,base,extend,payload,len);}
    else if(kind==GPU_BRUSH_FOUR_CORNER) {color=sample_four_corner(x,y,base,payload);}
    output[id.x]=color;
}
