#ifndef TILEINK_HLSL_COMPOSITE_STACK_INCLUDED
#define TILEINK_HLSL_COMPOSITE_STACK_INCLUDED
#include "../constants.hlsli"
#include "../draw_records.hlsli"
#include "../filter/config.hlsli"
#include "draw_tags.hlsli"
#include "stack_constants.hlsli"
#include "pixel.hlsli"
#include "blend.hlsli"
#include "layer_geometry.hlsli"

struct CompositeGroup { uint kind,parent_pixel,parent_clip,alpha,payload; };
uint composite_stack_pixel(ConstantBuffer<FilterConfig> config,
    ByteAddressBuffer draws, ByteAddressBuffer paths, ByteAddressBuffer backdrops,
    ByteAddressBuffer ranges, ByteAddressBuffer segments, ByteAddressBuffer paint,
    ByteAddressBuffer layers, uint destination, uint source, uint mask,
    uint2 xy, bool use_mask, bool force_blend) {
    uint pixel=destination, clip=255u, depth=0u;
    CompositeGroup groups[FILTER_GROUP_STACK_CAPACITY];
    for (uint index=config.layer_stack_start; index<config.layer_stack_end; ++index) {
        uint3 layer=layers.Load3(index*LAYER_RECORD_STRIDE);
        uint alpha=layer_alpha_at(draws,paths,backdrops,ranges,segments,paint,
            layer.y,config.paint_sdf_shadow_base,xy,uint2(config.tiles_width,config.tiles_height));
        if (layer.x==LAYER_CLIP) {clip=combine_alpha(clip,alpha);}
        else if ((layer.x==LAYER_OPACITY || layer.x==LAYER_BLEND) && depth<FILTER_GROUP_STACK_CAPACITY) {
            CompositeGroup group;
            group.kind=layer.x;group.parent_pixel=pixel;group.parent_clip=clip;
            group.alpha=alpha;group.payload=layer.z;
            groups[depth]=group;++depth;pixel=0u;
        }
    }
    uint source_alpha=use_mask?combine_alpha(clip,mask>>24u):clip;
    uint scaled=scale_premul_u8(source,source_alpha);
    if (force_blend) {
        if ((scaled>>24u)!=0u) pixel=blend_premul_u8(pixel,scaled,config.blend_mode);
    } else {pixel=src_over_premul_u8(pixel,scaled);}
    while (depth!=0u) {
        --depth;
        CompositeGroup group=groups[depth];
        uint alpha=combine_alpha(group.alpha,group.parent_clip);
        if (group.kind==LAYER_OPACITY) {
            alpha=combine_alpha(alpha,group.payload);
            pixel=src_over_premul_u8(group.parent_pixel,scale_premul_u8(pixel,alpha));
        } else {
            uint grouped_source=scale_premul_u8(pixel,alpha);
            pixel=(grouped_source>>24u)==0u?group.parent_pixel:
                blend_premul_u8(group.parent_pixel,grouped_source,group.payload);
        }
    }
    return pixel;
}
#endif
