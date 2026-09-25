#include <metal_stdlib>
using namespace metal;
#include "region.metal"
#include "../shared/pixel.metal"
float displacement_component(uint4 pixel,uint channel,bool linear) {
    if(channel==3) return float(pixel.a)/255.0f;
    float value=pixel.a?float(pixel[channel])/float(pixel.a):0.0f;
    return linear?(value<=0.04045f?value/12.92f:pow((value+0.055f)/1.055f,2.4f)):value;
}
kernel void filter_displacement_map_region(constant FilterConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],texture2d<float,access::read> map [[texture(2)]],
    texture2d<float,access::write> target [[texture(3)]],const device uint* tiles [[buffer(8)]],uint3 id [[thread_position_in_grid]]) {
    uint2 xy;if(!filter_position(config,tiles,id,xy)) return;
    uint4 pixel=byte_channels(pack_pixel(map.read(xy)));
    float2 displacement=float2(displacement_component(pixel,config.kernel_edge_mode,config.lighting_output_kind!=0),
        displacement_component(pixel,config.kernel_preserve_alpha,config.lighting_output_kind!=0))-0.5f;
    float2 position=rint(fma(displacement,float2(config.amount,config.rect_x0),float2(xy)));
    float4 result=0;
    if(all(position>=0) && all(position<float2(config.width,config.height))) result=source.read(uint2(position));
    target.write(result,xy);
}
