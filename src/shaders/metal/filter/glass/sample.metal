float4 glass_straight(float4 pixel) {if(pixel.a>0.000001f) pixel.rgb/=pixel.a;return pixel;}
float4 glass_sample(constant FilterConfig& c,texture2d<float,access::read> source,texture2d<float,access::read> auxiliary,bool blurred,float2 p) {
    p=clamp(p,0.0f,float2(c.width-1,c.height-1));
    if(blurred && c.downsample>1) {
        uint2 lower(c.source_x0,c.source_y0),upper(c.source_x1,c.source_y1);
        if(any(lower>=upper)) return float4(0);
        p=clamp((p+0.5f)/float(max(c.downsample,1u))-0.5f,float2(lower),float2(upper-1));
        if(!c.upsample_filter) return glass_straight(float4(byte_channels(pack_pixel(auxiliary.read(uint2(rint(p))))))/255.0f);
    }
    return glass_straight(blurred?sample_premul(auxiliary,uint2(c.width,c.height),p):sample_premul(source,uint2(c.width,c.height),p));
}
float glass_dispersion(constant FilterConfig& c,texture2d<float,access::read> source,texture2d<float,access::read> auxiliary,
    float2 p,float2 offset,float chromatic,uint channel,float amount) {
    float factor=1.0f-(chromatic-1.0f)*c.liquid_refraction_dispersion;
    float2 shift=0;if(factor!=0) shift=offset*factor;
    float a=glass_sample(c,source,auxiliary,false,p+shift)[channel],b=glass_sample(c,source,auxiliary,true,p+shift)[channel];
    return fma(b-a,amount,a);
}
