#include <metal_stdlib>
using namespace metal;
struct ShaderConfig {
    uint width,height,region_x0,region_y0,region_width,region_height;
    uint dispatch_width,pixel_count,compact_tiles,active_tile_count,tiles_width,tiles_height;
    float inverse_xx,inverse_yx,inverse_xy,inverse_yy,inverse_tx,inverse_ty;
    float sample_xx,sample_yx,sample_xy,sample_yy,sample_radius;
    uint uniform_words,output_x0,output_y0,output_width,output_height;
};
kernel void graph_channels(constant ShaderConfig& config [[buffer(0)]],
    texture2d<float,access::read> first [[texture(1)]],
    texture2d<float,access::read> second [[texture(2)]],
    texture2d<float,access::read> third [[texture(5)]],
    texture2d<float,access::write> output_image [[texture(3)]],uint2 p [[thread_position_in_grid]]) {
    if(any(p>=uint2(config.width,config.height))) return;
    output_image.write(float4(first.read(p).r,second.read(p).g,third.read(p).a,1),p);
}
float4 shifted(constant ShaderConfig& c,const device uint* parameters,
    texture2d<float,access::read> input_image,uint2 p) {
    float2 delta=as_type<float2>(uint2(parameters[0],parameters[1]));
    float det=c.inverse_xx*c.inverse_yy-c.inverse_xy*c.inverse_yx;
    delta=float2(c.inverse_yy*delta.x-c.inverse_xy*delta.y,
        -c.inverse_yx*delta.x+c.inverse_xx*delta.y)/det;
    int2 source=int2(p)-int2(round(delta));
    return all(source>=0)&&all(source<int2(c.width,c.height))?input_image.read(uint2(source)):float4(0);
}
kernel void offset_image(constant ShaderConfig& config [[buffer(0)]],
    texture2d<float,access::read> input_image [[texture(1)]],const device uint* parameters [[buffer(4)]],
    texture2d<float,access::write> output_image [[texture(3)]],uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    output_image.write(shifted(config,parameters,input_image,p),p);
}
kernel void shadow_image(constant ShaderConfig& config [[buffer(0)]],
    texture2d<float,access::read> input_image [[texture(1)]],const device uint* parameters [[buffer(4)]],
    texture2d<float,access::write> output_image [[texture(3)]],uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    float4 foreground=input_image.read(p);
    float alpha=shifted(config,parameters,input_image,p).a*(1-foreground.a);
    output_image.write(foreground+float4(0,0,0,alpha),p);
}
