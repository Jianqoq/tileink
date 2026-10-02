#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "../shared/layer.metal"
#include "../shared/blend/channels.metal"
#include "../shared/blend/compose.metal"
#include "stack_math.metal"
// The three entry points share the same scene ABI and stack evaluator.
kernel void filter_composite_stack_region(constant FilterConfig& config [[buffer(0)]],texture2d<float,access::read> source [[texture(1)]],     texture2d<float,access::read_write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],     const device uint* paint [[buffer(10)]],const device uint* draws [[buffer(20)]],const device uint* paths [[buffer(21)]],     const device uint* backdrops [[buffer(22)]],const device uint* ranges [[buffer(23)]],const device float* segments [[buffer(24)]],     const device uint* layers [[buffer(25)]],constant BufferSizes& sizes [[buffer(29)]],uint3 id [[thread_position_in_grid]],texture2d<float,access::read> mask [[texture(2)]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint result=composite_stack(config,LayerGeometry{Words{draws,word_size(sizes,20)},Words{paths,word_size(sizes,21)},Words{backdrops,word_size(sizes,22)},Words{ranges,word_size(sizes,23)},Words{paint,word_size(sizes,10)},segments},Words{layers,word_size(sizes,25)},pack_pixel(target.read(xy)),
        pack_pixel(source.read(xy)),pack_pixel(mask.read(xy)),xy,config.mask_enabled!=0,false);
    target.write(unpack_pixel(result),xy);
}
kernel void filter_composite_blend_stack_region(constant FilterConfig& config [[buffer(0)]],texture2d<float,access::read> source [[texture(1)]],     texture2d<float,access::read_write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],     const device uint* paint [[buffer(10)]],const device uint* draws [[buffer(20)]],const device uint* paths [[buffer(21)]],     const device uint* backdrops [[buffer(22)]],const device uint* ranges [[buffer(23)]],const device float* segments [[buffer(24)]],     const device uint* layers [[buffer(25)]],constant BufferSizes& sizes [[buffer(29)]],uint3 id [[thread_position_in_grid]],texture2d<float,access::read> mask [[texture(2)]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint result=composite_stack(config,LayerGeometry{Words{draws,word_size(sizes,20)},Words{paths,word_size(sizes,21)},Words{backdrops,word_size(sizes,22)},Words{ranges,word_size(sizes,23)},Words{paint,word_size(sizes,10)},segments},Words{layers,word_size(sizes,25)},pack_pixel(target.read(xy)),
        pack_pixel(source.read(xy)),pack_pixel(mask.read(xy)),xy,true,true);
    target.write(unpack_pixel(result),xy);
}
kernel void filter_composite_surface_stack_region(constant FilterConfig& config [[buffer(0)]],texture2d<float,access::read> source [[texture(1)]],     texture2d<float,access::read_write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],     const device uint* paint [[buffer(10)]],const device uint* draws [[buffer(20)]],const device uint* paths [[buffer(21)]],     const device uint* backdrops [[buffer(22)]],const device uint* ranges [[buffer(23)]],const device float* segments [[buffer(24)]],     const device uint* layers [[buffer(25)]],constant BufferSizes& sizes [[buffer(29)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    int2 position=int2(xy)-int2(config.offset_x,config.offset_y);
    if(any(position<0) || any(position>=int2(config.kernel_columns,config.kernel_rows))) return;
    uint result=composite_stack(config,LayerGeometry{Words{draws,word_size(sizes,20)},Words{paths,word_size(sizes,21)},Words{backdrops,word_size(sizes,22)},Words{ranges,word_size(sizes,23)},Words{paint,word_size(sizes,10)},segments},Words{layers,word_size(sizes,25)},pack_pixel(target.read(xy)),
        pack_pixel(source.read(uint2(position))),0,xy,false,false);
    target.write(unpack_pixel(result),xy);
}
