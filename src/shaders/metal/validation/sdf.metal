#include <metal_stdlib>
using namespace metal;
#include "../shared/buffer.metal"
#include "../filter/config.metal"
#include "../shared/sdf/coverage.metal"
kernel void sdf_coverage_words(constant FilterConfig& config [[buffer(0)]],
    const device uint* requests [[buffer(5)]], device uint* output [[buffer(6)]],
    const device uint* paint [[buffer(7)]], constant BufferSizes& sizes [[buffer(29)]],
    uint index [[thread_position_in_grid]]) {
    if (index >= config.pixel_count) return;
    const device uint* r = requests + index * 12;
    Affine inverse{as_type<float4>(uint4(r[4],r[5],r[6],r[7])),as_type<float2>(uint2(r[8],r[9]))};
    float2 local = affine_point(inverse, as_type<float2>(uint2(r[1],r[2])));
    float coverage = sdf_blob_coverage(Words{paint,word_size(sizes,7)},r[0],local,inverse);
    output[index] = uint(clamp(coverage,0.0f,1.0f)*255.0f+0.5f);
}
