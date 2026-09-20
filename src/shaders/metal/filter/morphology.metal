#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"

// Erosion/dilation operates on straight channels; transparent samples outside
// the surface participate in erosion rather than extending its edge color.
kernel void filter_morphology_axis_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::write> target [[texture(3)]],
    const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    bool horizontal=config.morphology_axis==0,dilate=config.morphology_operator==1;
    uint position=horizontal?xy.x:xy.y,length=horizontal?config.width:config.height,radius=config.morphology_radius;
    if(!dilate && (position<radius || position+radius>=length)) {target.write(float4(0),xy);return;}
    float4 result=dilate?float4(0):float4(1);
    uint first=position>radius?position-radius:0,last=min(position+radius,length-1);
    for(uint i=first;i<=last;++i) {
        uint4 pixel=byte_channels(pack_pixel(source.read(horizontal?uint2(i,xy.y):uint2(xy.x,i))));
        float4 value=float4(pixel.a?float3(pixel.rgb)/float(pixel.a):float3(0),float(pixel.a)/255.0f);
        result=dilate?max(result,value):min(result,value);
    }
    target.write(unpack_pixel(pack_pixel(float4(result.rgb*result.a,result.a))),xy);
}
