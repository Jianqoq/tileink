struct ShaderConfig {
    uint width,height,region_x0,region_y0,region_width,region_height;
    uint dispatch_width,pixel_count,compact_tiles,active_tile_count,tiles_width,tiles_height;
    float inverse_xx,inverse_yx,inverse_xy,inverse_yy,inverse_tx,inverse_ty;
    float sample_xx,sample_yx,sample_xy,sample_yy,sample_radius;
    uint uniform_words,output_x0,output_y0,output_width,output_height;
};
ConstantBuffer<ShaderConfig> config : register(b0);
Texture2D<float4> first : register(t1);
Texture2D<float4> input_image : register(t1);
Texture2D<float4> second : register(t2);
Texture2D<float4> third : register(t5);
ByteAddressBuffer parameters : register(t4);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> output_image : register(u3);
[numthreads(8,8,1)]
void graph_channels(uint2 p:SV_DispatchThreadID) {
    if(any(p>=uint2(config.width,config.height))) return;
    output_image[p]=float4(first.Load(int3(p,0)).r,second.Load(int3(p,0)).g,third.Load(int3(p,0)).a,1);
}
float4 shifted(uint2 p) {
    float2 delta=asfloat(parameters.Load2(0));
    float det=config.inverse_xx*config.inverse_yy-config.inverse_xy*config.inverse_yx;
    delta=float2(config.inverse_yy*delta.x-config.inverse_xy*delta.y,
        -config.inverse_yx*delta.x+config.inverse_xx*delta.y)/det;
    int2 source=int2(p)-int2(round(delta));
    return all(source>=0)&&all(source<int2(config.width,config.height))?input_image.Load(int3(source,0)):0;
}
[numthreads(8,8,1)]
void offset_image(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    output_image[p]=shifted(p);
}
[numthreads(8,8,1)]
void shadow_image(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    float4 foreground=input_image.Load(int3(p,0));
    float alpha=shifted(p).a*(1-foreground.a);
    output_image[p]=foreground+float4(0,0,0,alpha);
}
