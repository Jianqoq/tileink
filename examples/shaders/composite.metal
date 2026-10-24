#include <metal_stdlib>
using namespace metal;
struct ShaderConfig {
    uint width,height,region_x0,region_y0,region_width,region_height;
    uint dispatch_width,pixel_count,compact_tiles,active_tile_count,tiles_width,tiles_height;
    float inverse_xx,inverse_yx,inverse_xy,inverse_yy,inverse_tx,inverse_ty;
    float sample_xx,sample_yx,sample_xy,sample_yy,sample_radius;
    uint uniform_words,output_x0,output_y0,output_width,output_height;
};
kernel void composite(constant ShaderConfig& config [[buffer(0)]],
    texture2d<float,access::read> source [[texture(1)]],
    texture2d<float,access::read> backdrop [[texture(2)]],
    texture2d<float,access::write> output_image [[texture(3)]],uint2 p [[thread_position_in_grid]]) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    float4 a=source.read(p),b=backdrop.read(p);
    output_image.write(a+b*(1-a.a),p);
}
