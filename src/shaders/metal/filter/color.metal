float luminance(float3 rgb) {return (0.2126f*rgb.r+0.7152f*rgb.g)+0.0722f*rgb.b;}
float matrix_dot(float4 a,float4 b) {return fma(a.x,b.x,fma(a.y,b.y,fma(a.z,b.z,a.w*b.w)));}
uint color_filter(uint pixel,uint kind,float amount) {
    uint4 stored=byte_channels(pixel);
    if((kind==5 || kind==8) && stored.a!=0) {
        float3 input=float3(stored.rgb);
        float alpha=float(stored.a);
        float3 mapped=alpha-input;
        if(kind==8) mapped=float3(
            fma(0.393f,input.r,fma(0.769f,input.g,0.189f*input.b)),
            fma(0.349f,input.r,fma(0.686f,input.g,0.168f*input.b)),
            fma(0.272f,input.r,fma(0.534f,input.g,0.131f*input.b)));
        return pack_bytes(uint4(uint3(clamp(fma(mapped-input,float3(clamp(amount,0.0f,1.0f)),input),0.0f,alpha)+0.5f),stored.a));
    }
    float4 value=unpack_pixel(pixel);
    if(kind==6) value*=clamp(amount,0.0f,1.0f);
    else if(value.a>0) {
        float3 rgb=value.rgb/value.a;
        switch(kind) {
            case 1:rgb*=amount;break;
            case 2:rgb=(rgb-0.5f)*amount+0.5f;break;
            case 3:rgb=fma(float3(luminance(rgb))-rgb,float3(clamp(amount,0.0f,1.0f)),rgb);break;
            case 4:{
                float radians=amount*0.017453292f,c=cos(radians),s=sin(radians);
                float3 r=float3(0.213f+c*0.787f-s*0.213f,0.715f-c*0.715f-s*0.715f,0.072f-c*0.072f+s*0.928f);
                float3 g=float3(0.213f-c*0.213f+s*0.143f,0.715f+c*0.285f+s*0.140f,0.072f-c*0.072f-s*0.283f);
                float3 b=float3(0.213f-c*0.213f-s*0.787f,0.715f-c*0.715f+s*0.715f,0.072f+c*0.928f+s*0.072f);
                rgb=float3((r.x*rgb.x+r.y*rgb.y)+r.z*rgb.z,(g.x*rgb.x+g.y*rgb.y)+g.z*rgb.z,(b.x*rgb.x+b.y*rgb.y)+b.z*rgb.z);
                break;
            }
            case 7:{float y=luminance(rgb);rgb=y+(rgb-y)*amount;break;}
        }
        value.rgb=clamp(rgb,0.0f,1.0f)*value.a;
    }
    return pack_pixel(value);
}

// Matrix arithmetic stays in premultiplied byte units. Unchanged alpha has an
// exact scale of one; this avoids a reciprocal round trip at half-byte values.
uint matrix_filter(constant FilterConfig& config,uint pixel) {
    uint4 stored=byte_channels(pixel);
    float alpha=float(stored.a);
    float3 channels=float3(stored.rgb),straight=0;
    if(stored.a==255) straight=channels;
    else if(alpha>0) straight=channels*(255.0f/alpha);
    // Quantize output alpha before premultiplication so low-alpha RGB does not gain a byte.
    float output_alpha=floor(clamp(matrix_dot(config.matrix_a,float4(straight,alpha))+config.matrix_bias.w*255.0f,0.0f,255.0f)+0.5f);
    float3 result;
    if(alpha>0) {
        float4 input=float4(channels,alpha*alpha*(1.0f/255.0f));
        float3 mapped=float3(matrix_dot(config.matrix_r,input),matrix_dot(config.matrix_g,input),matrix_dot(config.matrix_b,input));
        mapped=fma(config.matrix_bias.rgb,float3(alpha),mapped);
        float scale=output_alpha==alpha?1.0f:output_alpha/alpha;
        result=select(clamp(mapped,0.0f,alpha)*scale,float3(output_alpha),mapped>=alpha);
    } else result=clamp(config.matrix_bias.rgb,0.0f,1.0f)*output_alpha;
    return pack_bytes(uint4(clamp(float4(result,output_alpha),0.0f,255.0f)+0.5f));
}
