#ifndef TILEINK_HLSL_BRUSH_SAMPLE_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_SAMPLE_HLSLI_INCLUDED
#include "constants.hlsli"
#include "data.hlsli"
#include "pattern.hlsli"
#include "../pattern_transform.hlsli"
#include "linear.hlsli"
#include "radial.hlsli"
#include "sweep.hlsli"
#include "four_corner.hlsli"
#include "texture.hlsli"
#include "../texture_table_constants.hlsli"

uint sample_brush(ByteAddressBuffer paint, uint brush_base, uint data_base,
    Texture2DArray<float4> atlas, SamplerState image_sampler,
    Texture2D<float4> images[NATIVE_TEXTURE_TABLE_CAPACITY], float x, float y) {
    uint kind=brush_word(paint,brush_base,data_base);
    uint extend=brush_word(paint,brush_base,data_base+1u);
    uint payload=data_base+brush_word(paint,brush_base,data_base+2u);
    uint len=brush_word(paint,brush_base,data_base+3u);
    uint base=data_base+BRUSH_HEADER_WORDS;
    uint color=brush_word(paint,brush_base,data_base+BRUSH_COLOR_WORD);
    if(kind==BRUSH_LINEAR) return sample_linear(paint,brush_base,x,y,base,extend,payload,len);
    if(kind==BRUSH_RADIAL) return sample_radial(paint,brush_base,x,y,base,extend,payload,len);
    if(kind==BRUSH_SWEEP) return sample_sweep(paint,brush_base,x,y,base,extend,payload,len);
    if(kind==BRUSH_FOUR_CORNER) return sample_four_corner(paint,brush_base,x,y,base,payload);
    if(kind==BRUSH_PATTERN) return 0u;
    if(kind!=BRUSH_PATTERN_RESOURCE) return color;
    uint placement=brush_word(paint,brush_base,data_base+4u);
    uint2 size=uint2(brush_word(paint,brush_base,data_base+5u),brush_word(paint,brush_base,data_base+6u));
    if(any(size==0u)) return 0u;
    if((placement & BRUSH_TEXTURE_PLACEMENT_BIT)==0u)
        return sample_atlas_brush(paint,brush_base,data_base,atlas,image_sampler,x,y);
    float tx=pattern_transform_component(brush_param(paint,brush_base,base,0u),brush_param(paint,brush_base,base,2u),brush_param(paint,brush_base,base,4u),x,y)*float(size.x);
    float ty=pattern_transform_component(brush_param(paint,brush_base,base,1u),brush_param(paint,brush_base,base,3u),brush_param(paint,brush_base,base,5u),x,y)*float(size.y);
    uint index=placement & BRUSH_TEXTURE_INDEX_MASK;
    if(index>=NATIVE_TEXTURE_TABLE_CAPACITY) return 0u;
    return sample_resource_pattern_texture(images[NonUniformResourceIndex(index)],image_sampler,float2(tx,ty),size,
        brush_word(paint,brush_base,data_base+7u),extend,brush_word(paint,brush_base,data_base+8u));
}
#endif
