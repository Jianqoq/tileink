#ifndef TILEINK_HLSL_FINE_DRAW_INCLUDED
#define TILEINK_HLSL_FINE_DRAW_INCLUDED
#include "inputs.hlsli"
#include "records.hlsli"
#include "../shared/affine.hlsli"
#include "../shared/brush/constants.hlsli"
#include "../shared/brush/data.hlsli"
#include "../shared/brush/sample.hlsli"
#include "../shared/texture_table_constants.hlsli"
#include "../shared/draw_tags.hlsli"
#include "../shared/sdf/coverage.hlsli"

bool pixel_in_draw_bounds(FineDraw draw,uint x,uint y) {
    return int(x)>=draw.pixel_bounds.x && int(y)>=draw.pixel_bounds.y && int(x)<draw.pixel_bounds.z && int(y)<draw.pixel_bounds.w;
}
uint sample_draw_brush(FineInputs input,Texture2D<float4> images[NATIVE_TEXTURE_TABLE_CAPACITY],FineDraw draw,float x,float y) {
    if(brush_word(input.paint,input.config.paint_brush_base,draw.brush_offset)==BRUSH_SOLID)
        return brush_word(input.paint,input.config.paint_brush_base,draw.brush_offset+BRUSH_COLOR_WORD);
    float2 local=affine_record_point(draw.inverse_transform,float2(x,y));
    return sample_brush(input.paint,input.config.paint_brush_base,draw.brush_offset,input.atlas,input.image_sampler,images,local.x,local.y);
}
float sdf_coverage_from_draw(FineInputs input,FineDraw draw,float x,float y) {
    float2 local=affine_record_point(draw.inverse_transform,float2(x,y));
    if(draw.sdf_offset!=INVALID_INDEX) return sdf_coverage_from_blob(input.paint,draw.sdf_offset,local.x,local.y,draw.inverse_transform);
    if(draw.shadow_offset!=INVALID_INDEX) return sdf_coverage_from_blob(input.paint,input.config.paint_sdf_shadow_base+draw.shadow_offset,local.x,local.y,draw.inverse_transform);
    return 0.0;
}

#endif
