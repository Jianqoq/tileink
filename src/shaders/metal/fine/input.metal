#include "../shared/tile_kinds.metal"
#include "config.metal"
struct FineInput {
    constant FineConfig* config;
    Words draws,paint,text;
    const device float* segments;
    const device uint* coarse;
    device uint* spills;
};
struct FineDraw {uint brush,sdf,shadow;int4 bounds;Affine inverse;};
FineDraw fine_draw(Words records,uint index) {
    Words p=records.offset(index*31);
    return {p[6],p[2],p[4],as_type<int4>(uint4(p[10],p[11],p[12],p[13])),
        Affine{as_type<float4>(uint4(p[25],p[26],p[27],p[28])),as_type<float2>(uint2(p[29],p[30]))}};
}
uint draw_brush(FineInput input,texture2d_array<float> atlas,sampler sampling,constant TextureTable& table,FineDraw draw,float2 position) {
    Words paint=input.paint.offset(input.config->paint_brush_base);
    if(paint[draw.brush]==1) return paint[draw.brush+4];
    return sample_brush(paint,draw.brush,atlas,sampling,table,affine_point(draw.inverse,position));
}
float draw_coverage(FineInput input,FineDraw draw,float2 position) {
    float2 local=affine_point(draw.inverse,position);
    if(draw.sdf!=0xffffffffu) return sdf_blob_coverage(input.paint,draw.sdf,local,draw.inverse);
    if(draw.shadow!=0xffffffffu) return sdf_blob_coverage(input.paint,input.config->paint_sdf_shadow_base+draw.shadow,local,draw.inverse);
    return 0;
}
struct Particle {uint tag;int backdrop;uint rule,start,end,color;};
Particle particle_at(FineInput input,uint index) {
    const device uint* p=input.coarse+input.config->tile_count*6+index*6;
    return {p[0],as_type<int>(p[1]),p[2],p[3],p[4],p[5]};
}

uint fine_tile_start(FineInput input,uint tile) {
    return input.coarse[tile*6+1] + (input.coarse[input.config->fine_tile_kind_base+tile] >> TILE_PREFIX_SHIFT);
}
