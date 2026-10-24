float srgb_linear(float value) {
    float v=clamp(value,0.0f,1.0f);
    return v>0.04045f?pow((v+0.055f)/1.055f,2.4f):v/12.92f;
}
float linear_srgb(float value) {
    float v=clamp(value,0.0f,1.0f);
    return v>0.0031308f?1.055f*pow(v,1.0f/2.4f)-0.055f:v*12.92f;
}
float srgb_derivative(float value) {
    float v=clamp(value,0.0f,1.0f);
    return v>0.0031308f?(1.055f/2.4f)*pow(v,1.0f/2.4f-1.0f):12.92f;
}
float3 linear_rgb(float3 c) {return float3(srgb_linear(c.r),srgb_linear(c.g),srgb_linear(c.b));}
float3 srgb_rgb(float3 c) {return float3(linear_srgb(c.r),linear_srgb(c.g),linear_srgb(c.b));}
float3 linear_premul(uint color,float alpha) {
    return alpha>0?linear_rgb(float3(byte_channels(color).rgb)*(1.0f/255.0f)/alpha)*alpha:float3(0);
}
uint pack_linear(float3 rgb,float a) {
    if(a<=0) return 0;
    a=clamp(a,0.0f,1.0f);
    uint3 channels=uint3(srgb_rgb(clamp(rgb/a,0.0f,1.0f))*a*255.0f+0.5f);
    return pack_bytes(uint4(channels,uint(a*255.0f+0.5f)));
}
float text_luma(float3 c) {return 0.2126f*c.r+0.7152f*c.g+0.0722f*c.b;}
struct TextColor {float alpha;float3 srgb,linear;float luma,perceptual,maximum,chroma;};
TextColor text_color(uint color) {
    float alpha=float(color>>24)*(1.0f/255.0f);
    float3 s=alpha>0?clamp(float3(byte_channels(color).rgb)*(1.0f/(alpha*255.0f)),0.0f,1.0f):float3(0);
    float3 l=linear_rgb(s);
    float hi=max(max(l.r,l.g),l.b),lo=min(min(l.r,l.g),l.b);
    return {alpha,s,l,text_luma(l),text_luma(s),hi,clamp(hi-lo,0.0f,1.0f)};
}
