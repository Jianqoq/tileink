// Complete application shader, compiled as supplied to DXC.
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
float4 border(int2 p) { return all(p>=0) && all(p<int2(config.width,config.height))?input_image.Load(int3(p,0)):float4(0,0,0,0); }
float4 sample_input(uint2 p,float2 offset) {
    offset=clamp(offset,-config.sample_radius,config.sample_radius);
    float2 q=float2(p)+float2(config.sample_xx*offset.x+config.sample_xy*offset.y,config.sample_yx*offset.x+config.sample_yy*offset.y);
    int2 a=int2(floor(q));float2 t=frac(q);
    return lerp(lerp(border(a),border(a+int2(1,0)),t.x),lerp(border(a+int2(0,1)),border(a+int2(1,1)),t.x),t.y);
}
float4 output_color(float4 v) {if(!all(isfinite(v))) return 0;float a=clamp(v.a,0.0,1.0);return floor(clamp(v,0.0,a)*255.0+0.5)*(1.0/255.0);}
[numthreads(256,1,1)]
void glass(uint3 group:SV_GroupID,uint lane:SV_GroupIndex) {
    uint2 p;if(!position(group,lane,p)) return;
    float2 pos=logical(p);
    float2 relative=pos-asfloat(uniforms.Load4(0)).xy;
    float2 q=abs(relative)-asfloat(uniforms.Load4(16)).xy;
    float2 outside=max(q,0.0);
    float len=length(outside);
    float distance=len+min(max(q.x,q.y),0.0)-asfloat(uniforms.Load4(32)).x;
    float2 axis=step(q.yx,q)*(len<=1e-6?1.0:0.0);
    float2 normal=(outside+axis)*relative/max(abs(relative),float2(1e-6,1e-6));
    normal/=max(length(normal),1e-6);
    float rim=clamp(1.0+distance*asfloat(uniforms.Load4(48)).x,0.0,1.0);
    float2 shift=normal*rim*asfloat(uniforms.Load4(64)).xy;
    float4 sampled=sample_input(p,shift);
    if(uniforms.Load(112)!=0) { sampled.r=sample_input(p,shift*0.96).r;sampled.b=sample_input(p,shift*1.04).b; }
    float fresnel=rim*rim;fresnel*=fresnel;
    float4 color=sampled*asfloat(uniforms.Load4(80))+fresnel*asfloat(uniforms.Load4(96));
    float4 result=lerp(input_image.Load(int3(p,0)),color,clamp(-distance,0.0,1.0));
    output_image[p-uint2(config.output_x0,config.output_y0)]=output_color(result);
}
