// Preserve product rounding before the nested fused lighting sums.
float light_dot(constant FilterConfig& c,float3 a,float3 b) {return fma(a.x,b.x,fma(a.y,b.y,fma(a.z,b.z,c.rounding_zero)));}
float light_power(float value,float exponent) {return exponent<=0?1.0f:pow(value,exponent);}
float light_alpha(texture2d<float,access::read> source,uint2 xy) {return float(pack_pixel(source.read(xy))>>24);}
// One-sided edge differences preserve the same derivative scale as interior
// pixels. The orthogonal Sobel weights include only samples in the primitive.
float light_gradient(constant FilterConfig& c,texture2d<float,access::read> source,uint2 xy,bool horizontal) {
    uint2 lower=uint2(c.region_x0,c.region_y0),size=uint2(c.region_width,c.region_height),upper=lower+size;
    uint length=horizontal?size.x:size.y;if(length<2) return 0;
    float sum=0,weights=0;
    uint p=horizontal?xy.x:xy.y,start=horizontal?lower.x:lower.y,end=start+length;
    for(int offset=-1;offset<=1;++offset) {
        int cross=int(horizontal?xy.y:xy.x)+offset;
        if(cross<int(horizontal?lower.y:lower.x) || cross>=int(horizontal?upper.y:upper.x)) continue;
        float weight=offset==0?2.0f:1.0f;
        uint before=p>start?p-1:p,after=p<end-1?p+1:p;
        uint2 a=horizontal?uint2(before,cross):uint2(cross,before),b=horizontal?uint2(after,cross):uint2(cross,after);
        sum+=weight*(light_alpha(source,b)-light_alpha(source,a));weights+=weight;
    }
    return sum*(p==start || p==end-1?2.0f:1.0f)/(255.0f*weights);
}
uint lighting_pixel(constant FilterConfig& c,texture2d<float,access::read> source,uint2 xy) {
    uint dark=c.lighting_output_kind==0?0xff000000u:0;
    float z=(light_alpha(source,xy)/255.0f)*c.surface_scale;
    float3 normal(-light_gradient(c,source,xy,true)*c.surface_scale,-light_gradient(c,source,xy,false)*c.surface_scale,1);
    float normal_length=sqrt(light_dot(c,normal,normal));
    float3 light(c.light_p0-(float(c.surface_origin_x)+float(xy.x)+0.5f),c.light_p1-(float(c.surface_origin_y)+float(xy.y)+0.5f),c.light_p2-z);
    float attenuation=1;
    if(c.light_kind==0) {
        float azimuth=c.light_p0*0.017453292f,elevation=c.light_p1*0.017453292f;
        light=float3(cos(azimuth)*cos(elevation),sin(azimuth)*cos(elevation),sin(elevation));
    } else {
        float length=sqrt(light_dot(c,light,light));if(length<=0.000001f) return dark;
        light/=length;
        if(c.light_kind==2) {
            float3 spot(c.light_p3-c.light_p0,c.light_p4-c.light_p1,c.light_p5-c.light_p2);
            float length=sqrt(light_dot(c,spot,spot));if(length<=0.000001f) return dark;
            spot/=length;
            float focus=-light_dot(c,light,spot);
            if(focus<0 || (c.light_p7>=0 && focus<cos(c.light_p7*0.017453292f))) return dark;
            attenuation=light_power(focus,c.light_p6);
        }
    }
    float3 color(c.light_r,c.light_g,c.light_b);
    if(c.lighting_output_kind==0) {
        float intensity=c.light_constant*attenuation*max(light_dot(c,normal/normal_length,light),0.0f);
        return pack_pixel(float4(clamp(color*intensity,0.0f,1.0f),1));
    }
    float3 halfway(light.xy,light.z+1.0f);float length=sqrt(light_dot(c,halfway,halfway));
    if(length<=0.000001f) return dark;
    float cosine=max(light_dot(c,normal,halfway)/(normal_length*length),0.0f);
    color=clamp(color*(c.light_constant*attenuation*light_power(cosine,c.specular_exponent)),0.0f,1.0f);
    return pack_pixel(float4(color,max(max(color.r,color.g),color.b)));
}
