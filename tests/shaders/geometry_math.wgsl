// Adapter declarations also satisfy the unused production fill_alpha_at helper.
struct Segment { p0x:f32,p0y:f32,p1x:f32,p1y:f32,y_edge:f32 }
@group(0) @binding(3) var<storage,read> segments:array<Segment>;
struct MathConfig { count:u32,pad0:u32,pad1:u32,pad2:u32 }
@group(0) @binding(0) var<uniform> config:MathConfig;
@group(0) @binding(1) var<storage,read> source:array<u32>;
@group(0) @binding(2) var<storage,read_write> destination:array<u32>;
@compute @workgroup_size(FINE_WORKGROUP_SIZE)
fn geometry_math_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if (id.x>=config.count) { return; }
    let base=id.x*8u;
    let points=vec4<f32>(bitcast<f32>(source[base]),bitcast<f32>(source[base+1u]),bitcast<f32>(source[base+2u]),bitcast<f32>(source[base+3u]));
    let edge=bitcast<f32>(source[base+4u]); let y=source[base+5u]; let x=source[base+6u]; let rule=source[base+7u];
    let parts=segment_row_parts(points.x,points.y,points.z,points.w,edge,y);
    let area=segment_area_at(parts.z,parts.w,x);
    let coordinate=pattern_transform_component(points.x,points.y,edge,points.z,points.w);
    destination[id.x*4u]=coverage_to_alpha(parts.x+area*parts.y,rule);
    destination[id.x*4u+1u]=coverage_to_u8(area);
    destination[id.x*4u+2u]=bitcast<u32>(i32(floor(coordinate)));
    destination[id.x*4u+3u]=coverage_to_alpha(parts.y,rule);
}

@compute @workgroup_size(FINE_WORKGROUP_SIZE)
fn fill_coverage_words(@builtin(global_invocation_id) id:vec3<u32>) {
    if (id.x>=config.count) { return; }
    let base=id.x*8u;
    destination[id.x]=fill_alpha_at(bitcast<i32>(source[base+2u]),source[base+5u],source[base],source[base+1u],source[base+3u],source[base+4u]);
}
