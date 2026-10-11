#ifndef TILEINK_HLSL_COMPOSITE_STACK_INCLUDED
#define TILEINK_HLSL_COMPOSITE_STACK_INCLUDED
#include "../constants.hlsli"
#include "../draw_records.hlsli"
#include "../filter/config.hlsli"
#include "pixel.hlsli"
#include "blend.hlsli"
#include "layer_geometry.hlsli"

// Offscreen stack ranges contain only clips; groups own separate surfaces.
uint composite_clip_stack_pixel(ConstantBuffer<FilterConfig> config,
    ByteAddressBuffer draws, ByteAddressBuffer paths, ByteAddressBuffer backdrops,
    ByteAddressBuffer ranges, ByteAddressBuffer segments, ByteAddressBuffer paint,
    ByteAddressBuffer layers, uint destination, uint source, uint mask,
    uint2 xy, bool use_mask, bool force_blend) {
    uint clip=255u;
    for (uint index=config.layer_stack_start; index<config.layer_stack_end; ++index) {
        uint draw=layers.Load(index*LAYER_RECORD_STRIDE+4u);
        uint alpha=layer_alpha_at(draws,paths,backdrops,ranges,segments,paint,
            draw,config.paint_sdf_shadow_base,xy,uint2(config.tiles_width,config.tiles_height));
        clip=combine_alpha(clip,alpha);
    }
    uint source_alpha=use_mask?combine_alpha(clip,mask>>24u):clip;
    uint scaled=scale_premul_u8(source,source_alpha);
    if (force_blend)
        return (scaled>>24u)!=0u?blend_premul_u8(destination,scaled,config.blend_mode):destination;
    return src_over_premul_u8(destination,scaled);
}
#endif
