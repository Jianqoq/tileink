@group(0) @binding(5) var<storage,read> sdf_requests:array<u32>;
@group(0) @binding(6) var<storage,read_write> sdf_output:array<u32>;
@compute @workgroup_size(FILTER_WORKGROUP_SIZE)
fn sdf_coverage_words(@builtin(global_invocation_id) gid:vec3<u32>) {
    if(gid.x>=config.pixel_count){return;}
    let base=gid.x*SDF_PROBE_REQUEST_WORDS;
    let affine=base+SDF_PROBE_AFFINE_WORD;
    let inverse=AffineRecord(bitcast<f32>(sdf_requests[affine+0u]),bitcast<f32>(sdf_requests[affine+1u]),bitcast<f32>(sdf_requests[affine+2u]),bitcast<f32>(sdf_requests[affine+3u]),bitcast<f32>(sdf_requests[affine+4u]),bitcast<f32>(sdf_requests[affine+5u]));
    let local_position=affine_record_point(inverse,vec2<f32>(bitcast<f32>(sdf_requests[base+1u]),bitcast<f32>(sdf_requests[base+2u])));
    sdf_output[gid.x]=coverage_to_u8(sdf_coverage_from_blob(sdf_requests[base],false,local_position.x,local_position.y,inverse));
}
