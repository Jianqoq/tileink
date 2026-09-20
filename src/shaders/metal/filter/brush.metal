#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/buffer.metal"
#include "../shared/pixel.metal"
#include "../shared/affine.metal"
#include "../shared/brush/gradient.metal"
#include "../shared/brush/pattern.metal"
kernel void filter_flood_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],const device uint* brushes [[buffer(10)]],
    texture2d_array<float> atlas [[texture(12)]],sampler sampling [[sampler(13)]],constant TextureTable& table [[buffer(30)]],
    constant BufferSizes& sizes [[buffer(29)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint color=sample_brush(Words{brushes,word_size(sizes,10)},config.brush_offset,atlas,sampling,table,float2(xy)+0.5f);
    target.write(unpack_pixel(color),xy);
}
kernel void filter_composite_drop_shadow_region(constant FilterConfig& config [[buffer(0)]],texture2d<float,access::read> mask [[texture(2)]],
    texture2d<float,access::read_write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],const device uint* brushes [[buffer(10)]],
    texture2d_array<float> atlas [[texture(12)]],sampler sampling [[sampler(13)]],constant TextureTable& table [[buffer(30)]],
    constant BufferSizes& sizes [[buffer(29)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint color=sample_brush(Words{brushes,word_size(sizes,10)},config.brush_offset,atlas,sampling,table,float2(xy)+0.5f);
    uint shadow=scale_pixel(color,pack_pixel(mask.read(xy))>>24);
    target.write(unpack_pixel(source_over(shadow,pack_pixel(target.read(xy)))),xy);
}
