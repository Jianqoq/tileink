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

kernel void fine_tile_main(constant FineConfig& config [[buffer(0)]],texture2d<float,access::read_write> target [[texture(1)]],
    const device uint* draws [[buffer(2)]],const device uint* paint [[buffer(3)]],device uint* coarse [[buffer(4)]],
    const device float* segments [[buffer(5)]],const device uint* text [[buffer(6)]],device uint* spills [[buffer(7)]],
    texture2d_array<float> atlas [[texture(12)]],sampler sampling [[sampler(13)]],constant TextureTable& table [[buffer(30)]],
    constant BufferSizes& sizes [[buffer(29)]],uint2 group [[threadgroup_position_in_grid]],uint lane [[thread_index_in_threadgroup]]) {
    uint dispatch=group.x+group.y*config.dispatch_width;
    if(dispatch>=config.active_tile_count) return;
    uint tile=config.incremental?coarse[config.active_tile_list_base+dispatch]:dispatch;
    uint2 tile_position(tile%config.tiles_width,tile/config.tiles_width),xy=tile_position*16+uint2(lane%16,lane/16);
    if(tile_position.y>=config.tiles_height || xy.x>=config.width || xy.y>=config.height) return;
    FineInput input{&config,Words{draws,word_size(sizes,2)},Words{paint,word_size(sizes,3)},Words{text,word_size(sizes,6)},segments,coarse,spills};
    float4 pixel=config.load_target?target.read(xy):unpack_pixel(config.clear_color);
    if(coarse[config.fine_tile_kind_base+tile]!=1) pixel=fine_pixel(input,atlas,sampling,table,pixel,tile,lane,xy);
    target.write(pixel,xy);
}
