#include <metal_stdlib>
using namespace metal;
#include "../shared/buffer.metal"
#include "../shared/pixel.metal"
#include "../shared/brush/gradient.metal"
#include "../shared/brush/pattern.metal"
#include "../fine/config.metal"
struct RequestConfig {uint count,pad0,pad1,pad2;};
kernel void brush_words(constant FineConfig& config [[buffer(0)]],
    const device uint* raw_paint [[buffer(3)]],const device uint* requests [[buffer(9)]],
    device uint* output [[buffer(10)]],constant RequestConfig& count [[buffer(11)]],
    texture2d_array<float> atlas [[texture(12)]],sampler sampling [[sampler(13)]],
    constant TextureTable& table [[buffer(30)]],constant BufferSizes& sizes [[buffer(29)]],
    uint index [[thread_position_in_grid]]) {
    if(index>=count.count) return;
    Words paint=Words{raw_paint,word_size(sizes,3)}.offset(config.paint_brush_base);
    const device uint* r=requests+index*4;
    output[index]=sample_brush(paint,r[0],atlas,sampling,table,as_type<float2>(uint2(r[1],r[2])));
}
