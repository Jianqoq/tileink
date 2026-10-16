// Application-owned HLSL. Entry points describe ordinary native compute work.
struct ShaderConfig {
    uint width,height,region_x0,region_y0,region_width,region_height;
    uint dispatch_width,pixel_count,compact_tiles,active_tile_count,tiles_width,tiles_height;
    float inverse_xx,inverse_yx,inverse_xy,inverse_yy,inverse_tx,inverse_ty;
    float sample_xx,sample_yx,sample_xy,sample_yy,sample_radius;
    uint uniform_words,output_x0,output_y0,output_width,output_height;
};
ConstantBuffer<ShaderConfig> config : register(b0);
Texture2D<float4> input_image : register(t1);
#ifdef __spirv__
[[vk::image_format("rgba16f")]]
#endif
RWTexture2D<float4> low_image : register(u2);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> output_image : register(u3);
ByteAddressBuffer parameters : register(t4);
ByteAddressBuffer weights : register(t4);
Texture2D<float4> tint : register(t5);
SamplerState linear_sampler : register(s6);
[numthreads(16,16,1)]
void reduce(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    uint factor=parameters.Load(0),count=0;float4 sum=0;
    for(uint y=0;y<factor;y++) for(uint x=0;x<factor;x++) {
        uint2 q=p*factor+uint2(x,y);
        if(all(q<uint2(config.output_width,config.output_height))) {sum+=input_image.Load(int3(q,0));count++;}
    }
    low_image[p]=sum/float(count);
}
float4 gaussian(uint2 p,uint axis) {
    int radius=int(weights.Load(0));float4 sum=0;
    for(int i=-radius;i<=radius;i++) {
        int2 q=clamp(int2(p)+(axis==0?int2(i,0):int2(0,i)),int2(0,0),int2(config.width-1,config.height-1));
        sum+=input_image.Load(int3(q,0))*asfloat(weights.Load((1+i+radius)*4));
    }
    return sum;
}
[numthreads(16,16,1)]
void horizontal(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);if(all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) low_image[p]=gaussian(p,0);}
[numthreads(16,16,1)]
void vertical(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);if(all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) low_image[p]=gaussian(p,1);}
[numthreads(16,16,1)]
void expand(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    uint w,h;input_image.GetDimensions(w,h);
    float2 uv=(float2(p)+0.5)/(float(parameters.Load(0))*float2(w,h));
    float4 v=input_image.SampleLevel(linear_sampler,uv,0)*tint.SampleLevel(linear_sampler,float2(0.5,0.5),0);
    output_image[p]=floor(clamp(v,0.0,v.a)*255.0+0.5)/255.0;
}
