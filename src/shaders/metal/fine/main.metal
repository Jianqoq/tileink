#include <metal_stdlib>
using namespace metal;
#include "../shared/buffer.metal"
#include "../shared/pixel.metal"
#include "../shared/coverage.metal"
#include "../shared/sdf/coverage.metal"
#include "../shared/brush/gradient.metal"
#include "../shared/brush/pattern.metal"
#include "../shared/blend/channels.metal"
#include "../shared/blend/compose.metal"
#include "text/gamma.metal"
#include "text/basic.metal"
#include "text/axis.metal"
#include "text/coverage.metal"
#include "input.metal"
#include "stack.metal"
#include "glyph.metal"
#include "interpreter.metal"
#include "specialized.metal"

// Full draws execute one thread per pixel in a 16x16 hardware tile. Sparse
// draws rasterize only active coverage tiles and fetch their destination color.
struct FineVertex { float4 position [[position]]; uint tile [[flat]]; };
vertex FineVertex fine_tile_vertex(constant FineConfig& config [[buffer(0)]],
    const device uint* coarse [[buffer(4)]],uint vertex_id [[vertex_id]],uint instance [[instance_id]]) {
    uint tile=coarse[config.active_tile_list_base+instance];
    uint2 corner(vertex_id&1,vertex_id>>1);
    float2 pixel=float2(uint2(tile%config.tiles_width,tile/config.tiles_width)*16+corner*16);
    return {float4(pixel/float2(config.width,config.height)*float2(2,-2)+float2(-1,1),0,1),tile};
}
struct FineColor { float4 color [[color(0)]]; };

fragment float4 fine_tile_sparse(FineVertex raster [[stage_in]],float4 destination [[color(0)]],
    constant FineConfig& config [[buffer(0)]],
    const device uint* draws [[buffer(2)]],const device uint* paint [[buffer(3)]],const device uint* coarse [[buffer(4)]],
    const device float* segments [[buffer(5)]],const device uint* text [[buffer(6)]],device uint* spills [[buffer(7)]],
    texture2d_array<float> atlas [[texture(12)]],sampler sampling [[sampler(13)]],constant TextureTable& table [[buffer(30)]],
    constant BufferSizes& sizes [[buffer(29)]]) {
    uint2 xy=uint2(raster.position.xy);
    uint tile=raster.tile,lane=(xy.y%16)*16+xy.x%16;
    FineInput input{&config,Words{draws,word_size(sizes,2)},Words{paint,word_size(sizes,3)},Words{text,word_size(sizes,6)},segments,coarse,spills};
    float4 pixel=config.load_target?destination:unpack_pixel(config.clear_color);
    return shade_tile_pixel(input,atlas,sampling,table,pixel,tile,lane,xy);
}

kernel void fine_tile_main(imageblock<FineColor,imageblock_layout_implicit> pixels,
    constant FineConfig& config [[buffer(0)]],
    const device uint* draws [[buffer(2)]],const device uint* paint [[buffer(3)]],const device uint* coarse [[buffer(4)]],
    const device float* segments [[buffer(5)]],const device uint* text [[buffer(6)]],device uint* spills [[buffer(7)]],
    texture2d_array<float> atlas [[texture(12)]],sampler sampling [[sampler(13)]],constant TextureTable& table [[buffer(30)]],
    constant BufferSizes& sizes [[buffer(29)]],uint2 group [[threadgroup_position_in_grid]],
    ushort2 local [[thread_position_in_threadgroup]]) {
    uint2 xy=group*16+uint2(local);
    if(xy.x>=config.width || xy.y>=config.height) return;
    uint tile=group.y*config.tiles_width+group.x,lane=local.y*16+local.x;
    FineInput input{&config,Words{draws,word_size(sizes,2)},Words{paint,word_size(sizes,3)},Words{text,word_size(sizes,6)},segments,coarse,spills};
    float4 pixel=config.load_target?pixels.read(local).color:unpack_pixel(config.clear_color);
    pixel=shade_tile_pixel(input,atlas,sampling,table,pixel,tile,lane,xy);
    pixels.write(FineColor{pixel},local);
}
