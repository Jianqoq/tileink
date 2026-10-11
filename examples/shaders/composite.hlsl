struct ShaderConfig {
    uint width,height,region_x0,region_y0,region_width,region_height;
    uint dispatch_width,pixel_count,compact_tiles,active_tile_count,tiles_width,tiles_height;
    float inverse_xx,inverse_yx,inverse_xy,inverse_yy,inverse_tx,inverse_ty;
    float sample_xx,sample_yx,sample_xy,sample_yy,sample_radius;
    uint uniform_words,output_x0,output_y0,output_width,output_height;
};
ConstantBuffer<ShaderConfig> config : register(b0);
Texture2D<float4> source : register(t1);
Texture2D<float4> backdrop : register(t2);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> output_image : register(u3);
[numthreads(8,8,1)]
void composite(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    float4 a=source.Load(int3(p,0)),b=backdrop.Load(int3(p,0));
    output_image[p]=a+b*(1-a.a);
}
