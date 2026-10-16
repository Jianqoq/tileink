float overlay_channel(float d,float s) {return d<=0.5f?2.0f*d*s:1.0f-2.0f*(1.0f-d)*(1.0f-s);}
float mix_channel(float d,float s,uint mode) {
    switch(mode) {
        case 1:return d*s;
        case 2:return d+s-d*s;
        case 3:return overlay_channel(d,s);
        case 4:return min(d,s);
        case 5:return max(d,s);
        case 6:return d==0?0:s<1?min(d/(1.0f-s),1.0f):1;
        case 7:return d==1?1:s>0?1.0f-min((1.0f-d)/s,1.0f):0;
        case 8:return overlay_channel(s,d);
        case 9: {
            if(s<=0.5f) return d-(1.0f-2.0f*s)*d*(1.0f-d);
            float curve=d<=0.25f?((16.0f*d-12.0f)*d+4.0f)*d:sqrt(d);
            return d+(2.0f*s-1.0f)*(curve-d);
        }
        case 10:return abs(d-s);
        case 11:return d+s-2.0f*d*s;
        default:return s;
    }
}
float luminosity(float3 c) {return fma(0.3f,c.r,fma(0.59f,c.g,0.11f*c.b));}
float saturation(float3 c) {return max(max(c.r,c.g),c.b)-min(min(c.r,c.g),c.b);}
float3 clip_color(float3 rgb) {
    float lum=luminosity(rgb),lo=min(min(rgb.r,rgb.g),rgb.b),hi=max(max(rgb.r,rgb.g),rgb.b);
    if(lo<0) rgb=lum+(rgb-lum)*lum/(lum-lo);
    if(hi>1) rgb=lum+(rgb-lum)*(1.0f-lum)/(hi-lum);
    return rgb;
}
float3 set_luminosity(float3 rgb,float lum) {return clip_color(rgb+(lum-luminosity(rgb)));}
float3 set_saturation(float3 rgb,float sat) {
    uint low=rgb.r<=rgb.g && rgb.r<=rgb.b?0:rgb.g<=rgb.b?1:2;
    uint high=rgb.r>=rgb.g && rgb.r>=rgb.b?0:rgb.g>=rgb.b?1:2;
    float3 result(0);
    if(low!=high) {
        uint middle=3-low-high;
        if(rgb[high]>rgb[low]) {result[middle]=(rgb[middle]-rgb[low])*sat/(rgb[high]-rgb[low]);result[high]=sat;}
    }
    return result;
}
float3 mix_color(float3 d,float3 s,uint mode) {
    if(mode==12) return set_luminosity(set_saturation(s,saturation(d)),luminosity(d));
    if(mode==13) return set_luminosity(set_saturation(d,saturation(s)),luminosity(d));
    if(mode==14) return set_luminosity(s,luminosity(d));
    if(mode==15) return set_luminosity(d,luminosity(s));
    return float3(mix_channel(d.r,s.r,mode),mix_channel(d.g,s.g,mode),mix_channel(d.b,s.b,mode));
}
