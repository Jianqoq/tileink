#include <metal_stdlib>
using namespace metal;
// This complete file belongs to the application. Tileink injects no source.
struct ShaderConfig {
    uint width,height,region_x0,region_y0,region_width,region_height;
    uint dispatch_width,pixel_count,compact_tiles,active_tile_count,tiles_width,tiles_height;
    float inverse_xx,inverse_yx,inverse_xy,inverse_yy,inverse_tx,inverse_ty;
    float sample_xx,sample_yx,sample_xy,sample_yy,sample_radius;
    uint uniform_words,output_x0,output_y0,output_width,output_height;
};
kernel void reduce(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::read> input_image [[texture(1)]], texture2d<float,access::write> low_image [[texture(2)]], const device uint* parameters [[buffer(4)]], uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    uint factor=parameters[0];float4 sum=0;uint count=0;
    for(uint y=0;y<factor;y++) for(uint x=0;x<factor;x++) {
        uint2 q=p*factor+uint2(x,y);
        if(all(q<uint2(config.output_width,config.output_height))) { sum+=input_image.read(q);count++; }
    }
    low_image.write(sum/float(count),p);
}
float4 gaussian(texture2d<float,access::read> input_image,constant ShaderConfig& config,const device uint* weights,uint2 p,uint axis) {
    int radius=int(weights[0]);float4 sum=0;
    for(int i=-radius;i<=radius;i++) {
        int2 q=int2(p)+(axis==0?int2(i,0):int2(0,i));
        q=clamp(q,int2(0),int2(config.width-1,config.height-1));
        sum+=input_image.read(uint2(q))*as_type<float>(weights[1+i+radius]);
    }
    return sum;
}
kernel void horizontal(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::read> input_image [[texture(1)]], texture2d<float,access::write> low_image [[texture(2)]], const device uint* weights [[buffer(4)]], uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) low_image.write(gaussian(input_image,config,weights,p,0),p);
}
kernel void vertical(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::read> input_image [[texture(1)]], texture2d<float,access::write> low_image [[texture(2)]], const device uint* weights [[buffer(4)]], uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) low_image.write(gaussian(input_image,config,weights,p,1),p);
}
kernel void expand(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::sample> input_image [[texture(1)]], texture2d<float,access::write> output_image [[texture(3)]], const device uint* parameters [[buffer(4)]], texture2d<float,access::sample> tint [[texture(5)]], sampler linear_sampler [[sampler(6)]], uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    float2 uv=(float2(p)+0.5f)/(float(parameters[0])*float2(input_image.get_width(),input_image.get_height()));
    float4 v=input_image.sample(linear_sampler,uv)*tint.sample(linear_sampler,float2(0.5f));
    output_image.write(floor(clamp(v,0.0f,v.a)*255.0f+0.5f)/255.0f,p);
}
