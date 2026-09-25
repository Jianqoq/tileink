#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
#include "../shared/layer.metal"
kernel void filter_layer_mask_region(constant FilterConfig& config [[buffer(0)]],texture2d<float,access::write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],const device uint* paint [[buffer(10)]],const device uint* draws [[buffer(20)]],
    const device uint* paths [[buffer(21)]],const device uint* backdrops [[buffer(22)]],const device uint* ranges [[buffer(23)]],
    const device float* segments [[buffer(24)]],constant BufferSizes& sizes [[buffer(29)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    LayerGeometry geometry{Words{draws,word_size(sizes,20)},Words{paths,word_size(sizes,21)},Words{backdrops,word_size(sizes,22)},
        Words{ranges,word_size(sizes,23)},Words{paint,word_size(sizes,10)},segments};
    uint alpha=layer_alpha(geometry,config.draw_ix,config.paint_sdf_shadow_base,xy,uint2(config.tiles_width,config.tiles_height));
    target.write(float4(float(alpha)*(1.0f/255.0f)),xy);
}
