uint glass_pixel(constant FilterConfig& c,texture2d<float,access::read> source,texture2d<float,access::read> auxiliary,uint2 xy,float distance) {
    uint base=pack_pixel(source.read(xy));float height=float(max(c.height,1u)),normalized=distance/height;
    if(normalized>=0.005f) return base;
    float2 p=float2(xy),normal=glass_normal(c,p+0.5f);
    float edge=glass_edge(-distance,c.liquid_refraction_thickness,c.liquid_refraction_factor);
    float blur=c.mask_enabled==1?1.0f:clamp(-distance/max(c.liquid_refraction_thickness,0.000001f),0.0f,1.0f);
    float4 color=glass_sample(c,source,auxiliary,true,p),tint(c.liquid_tint_r,c.liquid_tint_g,c.liquid_tint_b,1);
    float tint_mix=c.liquid_tint_a*0.8f,base_mix=c.liquid_tint_a*0.5f,normal_length=1414.2136f/height;
    if(edge<=0) {if(tint_mix>0) color=fma(tint-color,tint_mix,color);}
    else {
        float2 offset=0;
        if(normal.x!=0) offset.x=-normal.x*edge*70.71068f;
        if(normal.y!=0) offset.y=-normal.y*edge*70.71068f;
        float4 a=glass_sample(c,source,auxiliary,false,p+offset),b=glass_sample(c,source,auxiliary,true,p+offset);
        color=float4(fma(b.rgb-a.rgb,blur,a.rgb),max(a.a,b.a));
        if(abs(c.liquid_refraction_dispersion)>0.000001f) {
            color.r=glass_dispersion(c,source,auxiliary,p,offset,0.98f,0,blur);
            color.b=glass_dispersion(c,source,auxiliary,p,offset,1.02f,2,blur);
        }
        float3 blurred=color.rgb;
        if(tint_mix>0) color=fma(tint-color,tint_mix,color);
        if(c.liquid_fresnel_factor>0) {
            float fresnel=glass_highlight(distance,c.liquid_fresnel_range,c.liquid_fresnel_hardness);
            float3 lch=glass_lch(fma(tint.rgb-1.0f,base_mix,1.0f));
            lch.x=clamp(lch.x+20.0f*fresnel*c.liquid_fresnel_factor,0.0f,100.0f);
            float amount=fresnel*c.liquid_fresnel_factor*0.7f*normal_length;
            color=fma(float4(glass_rgb(lch),1)-color,amount,color);
        }
        if(c.liquid_glare_factor>0) {
            float geometry=glass_highlight(distance,c.liquid_glare_range,c.liquid_glare_hardness),angle=glass_glare(c,normal);
            float3 lch=glass_lch(fma(tint.rgb-blurred,base_mix,blurred));
            lch.x=clamp(lch.x+150.0f*angle*geometry,0.0f,120.0f);lch.y+=30.0f*angle*geometry;
            color=fma(float4(glass_rgb(lch),1)-color,angle*geometry*normal_length,color);
        }
    }
    float t=clamp((normalized+0.001f)/0.002f,0.0f,1.0f),amount=t*t*(3.0f-2.0f*t);
    color=fma(glass_straight(float4(byte_channels(base))/255.0f)-color,amount,color);
    color=clamp(color,0.0f,1.0f);
    return pack_pixel(float4(color.rgb*color.a,color.a));
}
