#include <metal_stdlib>
using namespace metal;
vertex float4 present_vertex(uint id [[vertex_id]]) {
    return float4(id == 1 ? 3.0f : -1.0f, id == 2 ? 3.0f : -1.0f, 0.0f, 1.0f);
}
fragment float4 present_fragment(float4 position [[position]], texture2d<float, access::read> source [[texture(0)]]) {
    return source.read(uint2(position.xy));
}
