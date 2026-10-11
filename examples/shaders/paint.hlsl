// Complete application shader, compiled as supplied to DXC.
struct ShaderConfig {
    uint width,height,region_x0,region_y0,region_width,region_height;
    uint dispatch_width,pixel_count,compact_tiles,active_tile_count,tiles_width,tiles_height;
    float inverse_xx,inverse_yx,inverse_xy,inverse_yy,inverse_tx,inverse_ty;
    float sample_xx,sample_yx,sample_xy,sample_yy,sample_radius;
    uint uniform_words,output_x0,output_y0,output_width,output_height;
};
ConstantBuffer<ShaderConfig> config : register(b0);
#ifdef __spirv__
[[vk::image_format("rgba8")]]
#endif
RWTexture2D<float4> output_image : register(u3);
ByteAddressBuffer active_tiles : register(t8);
ByteAddressBuffer uniforms : register(t10);
bool position(uint3 group,uint lane,out uint2 p) {
    uint index=(group.x+group.y*config.dispatch_width)*256+lane;p=0;
    if(index>=config.pixel_count) return false;
    if(config.compact_tiles) { uint tile=active_tiles.Load((index/256)*4);p=uint2(tile%config.tiles_width,tile/config.tiles_width)*16+uint2(index%16,(index/16)%16); }
    else p=uint2(config.region_x0+index%config.region_width,config.region_y0+index/config.region_width);
    return all(p<uint2(config.width,config.height)) && all(p>=uint2(config.region_x0,config.region_y0)) && all(p<uint2(config.region_x0+config.region_width,config.region_y0+config.region_height));
}
float2 logical(uint2 p) {float2 q=float2(p)+0.5;return float2(config.inverse_xx*q.x+config.inverse_xy*q.y+config.inverse_tx,config.inverse_yx*q.x+config.inverse_yy*q.y+config.inverse_ty);}
float4 output_color(float4 v) {if(!all(isfinite(v))) return 0;float a=clamp(v.a,0.0,1.0);return floor(clamp(v,0.0,a)*255.0+0.5)*(1.0/255.0);}
[numthreads(256,1,1)]
void paint(uint3 group:SV_GroupID,uint lane:SV_GroupIndex) {
    uint2 p;if(!position(group,lane,p)) return;
    float2 pos=logical(p);
    float2 uv=pos*asfloat(uniforms.Load4(0)).xy;
    float t=sin(uv.x+sin(uv.y)+asfloat(uniforms.Load4(16)).x)*0.5+0.5;
    float4 result=lerp(float4(0.04,0.22,0.72,1.0),float4(0.95,0.24,0.35,1.0),t);
    output_image[p-uint2(config.output_x0,config.output_y0)]=output_color(result);
}
