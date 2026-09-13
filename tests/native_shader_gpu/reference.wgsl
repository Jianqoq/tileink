struct Params {
    count:u32,
    source_offset:u32,
    destination_offset:u32,
    stride:u32,
    value:vec4<u32>,
}
@group(0) @binding(0) var<storage,read_write> destination:array<u32>;
@group(0) @binding(1) var<storage,read> source:array<u32>;
@group(0) @binding(2) var<uniform> params:Params;

@compute @workgroup_size(64)
fn clear_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x<params.count {destination[params.destination_offset/4+id.x]=params.value.x;}
}
@compute @workgroup_size(64)
fn copy_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x<params.count {destination[params.destination_offset/4+id.x]=source[params.source_offset/4+id.x];}
}
@compute @workgroup_size(64)
fn layout_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x<params.count {
        let values=params.value+vec4<u32>(id.x,params.source_offset,params.stride,params.count);
        let offset=(params.destination_offset+id.x*params.stride)/4;
        for(var lane=0u;lane<4u;lane++) {destination[offset+lane]=values[lane];}
    }
}

@compute @workgroup_size(64)
fn sample_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x >= params.count {return;}
    let position=f32(id.x)*bitcast<f32>(params.value.y)+bitcast<f32>(params.value.x);
    let base=floor(position);
    let fraction=position-base;
    let left=u32(clamp(base,0.0,f32(params.value.z-1u)));
    let right=u32(clamp(base+1.0,0.0,f32(params.value.z-1u)));
    let a=source[params.source_offset/4u+left];
    let b=source[params.source_offset/4u+right];
    var packed=0u;
    for(var lane=0u;lane<4u;lane++) {
        let low=f32((a>>(lane*8u))&255u);
        let high=f32((b>>(lane*8u))&255u);
        let value=low+(high-low)*fraction;
        packed |= u32(floor(value+0.5))<<(lane*8u);
    }
    destination[params.destination_offset/4u+id.x]=packed;
}
