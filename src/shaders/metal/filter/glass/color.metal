float glass_linear(float x) {return x>0.04045f?pow((x+0.055f)/1.055f,2.4f):x/12.92f;}
float glass_compand(float x) {return x>0.0031308f?1.055f*pow(x,0.41666666f)-0.055f:12.92f*x;}
float lab_curve(float x) {return x>0.008856452f?pow(x,0.33333334f):7.787037f*x+0.13793103f;}
float lab_inverse(float x) {return x>0.206897f?x*x*x:0.12841855f*(x-0.13793103f);}
// D65 LCH preserves the glass model's lightness/chroma highlights. Keep the
// scalar matrix evaluation order; dot() permits different contraction choices.
float3 glass_lch(float3 color) {
    float r=glass_linear(color.r),g=glass_linear(color.g),b=glass_linear(color.b);
    float x=lab_curve((r*0.4124f+g*0.3576f+b*0.1805f)/0.9504559f);
    float y=lab_curve(r*0.2126f+g*0.7152f+b*0.0722f);
    float z=lab_curve((r*0.0193f+g*0.1192f+b*0.9505f)/1.0890578f);
    float a=500.0f*(x-y),bb=200.0f*(y-z);
    return float3(116.0f*y-16.0f,sqrt(a*a+bb*bb),atan2(bb,a)*57.29578f);
}
float3 glass_rgb(float3 lch) {
    float hue=lch.z*0.017453292f,w=(lch.x+16.0f)/116.0f;
    float x=0.9504559f*lab_inverse(w+(lch.y*cos(hue))/500.0f),y=lab_inverse(w),z=1.0890578f*lab_inverse(w-(lch.y*sin(hue))/200.0f);
    return float3(glass_compand(x*3.2406255f+y*-1.537208f+z*-0.4986286f),
        glass_compand(x*-0.9689307f+y*1.8757561f+z*0.0415175f),
        glass_compand(x*0.0557101f+y*-0.2040211f+z*1.0569959f));
}
