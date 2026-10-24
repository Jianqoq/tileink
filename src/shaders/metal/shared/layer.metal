#include "draw.metal"
#include "coverage.metal"
#include "sdf/coverage.metal"
struct LayerGeometry { Words draws,paths,backdrops,ranges,paint;const device float* segments; };
uint layer_alpha(LayerGeometry geometry,uint draw_index,uint shadow_base,uint2 xy,uint2 dimensions) {
    if(draw_index>=geometry.draws.length/31) return 0;
    DrawData draw=load_draw(geometry.draws,draw_index);
    if(has_sdf(draw)) {
        if(any(int2(xy)<draw.bounds.xy) || any(int2(xy)>=draw.bounds.zw)) return 0;
        Words p=geometry.draws.offset(draw_index*31+25);
        Affine inverse{as_type<float4>(uint4(p[0],p[1],p[2],p[3])),as_type<float2>(uint2(p[4],p[5]))};
        uint offset=draw.sdf!=invalid_index?draw.sdf:shadow_base+draw.shadow;
        return coverage_u8(sdf_blob_coverage(geometry.paint,offset,affine_point(inverse,float2(xy)+0.5f),inverse));
    }
    uint index=draw_backdrop(geometry.paths,draw,xy/16,dimensions);
    if(index==invalid_index) return 0;
    return fill_alpha(geometry.segments,as_type<int>(geometry.backdrops[index]),draw.fill_rule,
        geometry.ranges[index*2],geometry.ranges[index*2+1],xy%16);
}
