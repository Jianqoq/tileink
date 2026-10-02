uint axis_coverage(float coverage,TextColor s,TextColor d,float contrast,float strength,float limit) {
    uint result=coverage_u8(coverage);
    if(strength<=0 || limit<=0) return result;
    float luma_gate=clamp((limit-contrast)/limit,0.0f,1.0f),chroma_gate=clamp(max(s.chroma,d.chroma),0.0f,1.0f);
    if(luma_gate<=0 || chroma_gate<=0) return result;
    float3 axis=s.srgb-d.srgb;
    float denominator=axis.r*axis.r+axis.g*axis.g+axis.b*axis.b;
    if(denominator<=0.000001f) return result;
    float clamped=clamp(coverage,0.0f,1.0f);
    float3 delta=s.linear-d.linear,mixed=d.linear+delta*clamped,color=srgb_rgb(mixed);
    float projection=clamp(((color.r-d.srgb.r)*axis.r+(color.g-d.srgb.g)*axis.g+(color.b-d.srgb.b)*axis.b)/denominator,0.0f,1.0f);
    float derivative=max((axis.r*srgb_derivative(mixed.r)*delta.r+axis.g*srgb_derivative(mixed.g)*delta.g+axis.b*srgb_derivative(mixed.b)*delta.b)/denominator,0.0f);
    float correction=0;
    if(derivative>0.0001f) correction=clamp(strength*luma_gate*chroma_gate,0.0f,1.0f)*(clamped-projection)/clamp(derivative,0.2f,5.0f);
    if(!(correction<0 && contrast<limit*0.05f)) result=coverage_u8(clamped+correction);
    return result;
}
uint axis_mask(uint destination,uint source,uint mask,float strength,float limit) {
    if(strength<=0 || limit<=0) return mask;
    TextColor s=text_color(source),d=text_color(destination);
    float chroma_gate=clamp(max(s.chroma,d.chroma),0.0f,1.0f);
    float luma_gate=clamp((limit-abs(s.perceptual-d.perceptual))/limit,0.0f,1.0f);
    if(luma_gate<=0 || chroma_gate<=0) return mask;
    float3 axis=s.srgb-d.srgb;
    float denominator=axis.r*axis.r+axis.g*axis.g+axis.b*axis.b;
    if(denominator<=0.000001f) return mask;
    TextColor rendered=text_color(linear_subpixel_over(destination,source,mask,255));
    float3 delta=rendered.srgb-d.srgb;
    float projection=clamp((delta.r*axis.r+delta.g*axis.g+delta.b*axis.b)/denominator,0.0f,1.0f);
    uint3 m=byte_channels(mask).rgb;
    float target=float(m.r+m.g+m.b)*(1.0f/765.0f);
    float correction=strength*luma_gate*chroma_gate*max(target-projection,0.0f);
    return pack_bytes(uint4(uint3(clamp(float3(m)*(1.0f/255.0f)+correction,0.0f,1.0f)*255.0f+0.5f),0));
}
