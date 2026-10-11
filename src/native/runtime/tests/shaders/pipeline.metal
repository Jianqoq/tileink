#include <metal_stdlib>
using namespace metal;
struct ShaderConfig {
    uint width,height,region_x0,region_y0,region_width,region_height;
    uint dispatch_width,pixel_count,compact_tiles,active_tile_count,tiles_width,tiles_height;
    float inverse_xx,inverse_yx,inverse_xy,inverse_yy,inverse_tx,inverse_ty;
    float sample_xx,sample_yx,sample_xy,sample_yy,sample_radius;
    uint uniform_words,output_x0,output_y0,output_width,output_height;
};
kernel void fill_buffer(device uint* storage [[buffer(4)]], uint3 p [[thread_position_in_grid]]) {
    uint i=p.x+4*p.y+16*p.z;
    storage[i]+=i+1;
}
kernel void down_hdr(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::read> input_image [[texture(1)]], texture2d<float,access::write> intermediate [[texture(2)]], uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    float4 v=input_image.read(min(p*2,uint2(config.output_width-1,config.output_height-1)));
    intermediate.write(v+float4(1.5f,-0.25f,0,0),p);
}
kernel void copy_hdr(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::read> input_image [[texture(1)]], texture2d<float,access::write> intermediate [[texture(2)]], uint2 p [[thread_position_in_grid]], uint lane [[thread_index_in_threadgroup]]) {
    p+=uint2(config.region_x0,config.region_y0);
    threadgroup float4 values[64];
    bool active=all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height));
    values[lane]=active?input_image.read(p):float4(0);
    threadgroup_barrier(mem_flags::mem_threadgroup);
    if(active) intermediate.write(values[lane],p);
}
kernel void finish_hdr(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::read> input_image [[texture(1)]], texture2d<float,access::write> output_image [[texture(3)]], const device uint* storage [[buffer(4)]], texture2d<float,access::sample> tint [[texture(5)]], sampler linear_sampler [[sampler(6)]], uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    float4 v=input_image.read(p/2)-float4(1.75f,-0.25f,0,0);
    float factor=float(storage[(p.x+p.y*config.width)%64]%4+1)*0.25f;
    v.r*=factor*tint.sample(linear_sampler,float2(0.5f)).r;
    output_image.write(floor(clamp(v,0.0f,1.0f)*255.0f+0.5f)/255.0f,p);
}
kernel void inplace_hdr(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::read_write> intermediate [[texture(2)]], uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) intermediate.write(intermediate.read(p)+float4(0.25f,0,0,0),p);
}
kernel void global_color(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::read> input_image [[texture(1)]], texture2d<float,access::write> output_image [[texture(3)]], uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) output_image.write(input_image.read(uint2(0)),p);
}
kernel void split_hdr(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::read> input_image [[texture(1)]], texture2d<float,access::write> intermediate [[texture(2)]], texture2d<float,access::write> alternate [[texture(7)]], uint2 p [[thread_position_in_grid]]) {
    if(all(p<uint2(config.width,config.height))) {float4 v=input_image.read(p);intermediate.write(v+float4(1.5f,0,0,0),p);alternate.write(v+float4(0,-0.25f,0,0),p);}
}
kernel void join_hdr(constant ShaderConfig& config [[buffer(0)]], texture2d<float,access::read> input_image [[texture(1)]], texture2d<float,access::read> alternate_read [[texture(7)]], texture2d<float,access::write> output_image [[texture(3)]], uint2 p [[thread_position_in_grid]]) {
    if(all(p<uint2(config.width,config.height))) {float4 a=input_image.read(p),b=alternate_read.read(p);output_image.write(float4(a.r-1.5f,b.g+0.25f,a.b,b.a),p);}
}
