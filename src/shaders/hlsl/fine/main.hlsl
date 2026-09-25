#include "config.hlsli"
#include "inputs.hlsli"
#include "../constants.hlsli"
#include "../coarse/tags.hlsli"
#include "../shared/texture_table_constants.hlsli"
#include "pixel.hlsli"
#include "interpreter.hlsli"
#include "specialized.hlsli"
ConstantBuffer<FineConfig> config : register(b0);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> target : register(u1);
ByteAddressBuffer draws : register(t2);
ByteAddressBuffer paint : register(t3);
RWByteAddressBuffer coarse : register(u4);
ByteAddressBuffer segments : register(t5);
ByteAddressBuffer text : register(t6);
RWByteAddressBuffer spills : register(u7);
Texture2DArray<float4> atlas : register(t12);
SamplerState image_sampler : register(s13);
Texture2D<float4> images[NATIVE_TEXTURE_TABLE_CAPACITY] : register(t30);
[numthreads(FINE_WORKGROUP_SIZE,1,1)]
void fine_tile_main(uint3 group : SV_GroupID,uint3 local : SV_GroupThreadID) {
    uint dispatch_ix=group.x+group.y*config.dispatch_width;
    if(dispatch_ix>=config.active_tile_count) return;
    uint tile_ix=config.incremental!=0u ? coarse.Load((config.active_tile_list_base+dispatch_ix)*4u) : dispatch_ix;
    uint2 tile=uint2(tile_ix%config.tiles_width,tile_ix/config.tiles_width);
    uint2 xy=tile*TILE_SIZE+uint2(local.x%TILE_SIZE,local.x/TILE_SIZE);
    if(tile.y>=config.tiles_height || xy.x>=config.width || xy.y>=config.height) return;
    FineInputs input;input.config=config;input.draws=draws;input.paint=paint;input.coarse=coarse;input.segments=segments;
    input.text=text;input.spills=spills;input.target=target;input.atlas=atlas;input.image_sampler=image_sampler;
    uint kind=coarse.Load((config.fine_tile_kind_base+tile_ix)*4u);
    float4 pixel=0.0;
    bool handled=false;
    if(kind==TILE_KIND_EMPTY) { pixel=fine_initial_pixel(input,xy.x,xy.y);handled=true; }
    else if(kind==TILE_KIND_COLOR) handled=color_only_no_stack_tile_pixel(input,tile_ix,local.x,pixel);
    else if(kind==TILE_KIND_SDF || kind==TILE_KIND_MIXED) handled=analytic_solid_no_stack_tile_pixel(input,images,tile_ix,local.x,pixel);
    if(!handled) pixel=tile_pixel(input,images,tile_ix,local.x);
    target[xy]=pixel;
}
