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
RWTexture2D<float4> intermediate : register(u2);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> output_image : register(u3);
RWByteAddressBuffer storage : register(u4);
// Different entry points may use the same slot with a different access contract.
ByteAddressBuffer storage_read : register(t4);
Texture2D<float4> tint : register(t5);
SamplerState linear_sampler : register(s6);
[numthreads(2,2,2)]
void fill_buffer(uint3 p:SV_DispatchThreadID) {
    uint i=p.x+4*p.y+16*p.z;storage.Store(i*4,storage.Load(i*4)+i+1);
}
[numthreads(8,8,1)]
void down_hdr(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    intermediate[p]=input_image.Load(int3(min(p*2,uint2(config.output_width-1,config.output_height-1)),0))+float4(1.5,-0.25,0,0);
}
groupshared float4 values[64];
[numthreads(8,8,1)]
void copy_hdr(uint2 p:SV_DispatchThreadID,uint lane:SV_GroupIndex) {
    p+=uint2(config.region_x0,config.region_y0);
    bool active=all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height));
    values[lane]=active?input_image.Load(int3(p,0)):0;
    GroupMemoryBarrierWithGroupSync();
    if(active) intermediate[p]=values[lane];
}
[numthreads(8,8,1)]
void finish_hdr(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);
    if(any(p>=uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) return;
    float4 v=input_image.Load(int3(p/2,0))-float4(1.75,-0.25,0,0);
    float factor=float(storage_read.Load(((p.x+p.y*config.width)%64)*4)%4+1)*0.25;
    v.r*=factor*tint.SampleLevel(linear_sampler,float2(0.5,0.5),0).r;
    output_image[p]=floor(clamp(v,0.0,1.0)*255.0+0.5)/255.0;
}
[numthreads(8,8,1)]
void inplace_hdr(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);
    if(all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) intermediate[p]=intermediate[p]+float4(0.25,0,0,0);
}
[numthreads(8,8,1)]
void global_color(uint2 p:SV_DispatchThreadID) {
    p+=uint2(config.region_x0,config.region_y0);
    if(all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height))) output_image[p]=input_image.Load(int3(0,0,0));
}
#ifdef __spirv__
[[vk::image_format("rgba16f")]]
#endif
RWTexture2D<float4> alternate : register(u7);
Texture2D<float4> alternate_read : register(t7);
[numthreads(8,8,1)]
void split_hdr(uint2 p:SV_DispatchThreadID) {
    if(all(p<uint2(config.width,config.height))) {float4 v=input_image.Load(int3(p,0));intermediate[p]=v+float4(1.5,0,0,0);alternate[p]=v+float4(0,-0.25,0,0);}
}
[numthreads(8,8,1)]
void join_hdr(uint2 p:SV_DispatchThreadID) {
    if(all(p<uint2(config.width,config.height))) {float4 a=input_image.Load(int3(p,0)),b=alternate_read.Load(int3(p,0));output_image[p]=float4(a.r-1.5,b.g+0.25,a.b,b.a);}
}
