// Perceptual text tuning follows the shared alpha/LCD coverage contract. The
// zero source-chroma boost is omitted algebraically; alpha and LCD keep distinct
// low-luma and apparent-axis corrections before their final u8 quantization.
uint auto_coverage(uint destination,uint source,uint coverage,bool lcd) {
    if(!coverage || coverage==255) return coverage;
    TextColor s=text_color(source),d=text_color(destination);
    float chroma_scale=lcd?0.9483659f:1.3566802f;
    float low_limit=lcd?0.06385561f:0.10123872f;
    float reduction=lcd?0.0f:0.41302064f*max(1.3566802f-0.9483659f,0.0f);
    float axis_strength=lcd?0.0f:1.036124f,axis_limit=lcd?0.1682842f:0.6887328f;
    float3 contrast_rgb=abs(s.linear-d.linear);
    float channel=max(max(contrast_rgb.r,contrast_rgb.g),contrast_rgb.b),luma=abs(s.luma-d.luma);
    float low=clamp((low_limit-luma)/low_limit,0.0f,1.0f);low*=low;
    float suppression=reduction*low*(s.perceptual>=d.perceptual?1.0f:0.0f)*channel*clamp((s.chroma+d.chroma)*0.5f,0.0f,1.0f);
    float exponent;
    if(s.luma<d.luma) {
        float contrast=clamp(d.luma-s.luma,0.0f,1.0f),hidden=max(channel-contrast,0.0f);
        float dominance=clamp((d.chroma-s.chroma)*2.0f,0.0f,1.0f);
        float chroma=lcd?max(s.chroma,d.chroma*(1.0f-s.maximum)):s.chroma;
        float curve=clamp(contrast*(1.5728465f-1.15f*d.luma)+0.3656558f*chroma_scale*hidden*chroma,0.0f,1.0f);
        exponent=max(1.0f-0.95f*curve+suppression*dominance,0.03f);
    } else {
        float contrast=clamp(s.luma-d.luma,0.0f,1.0f);
        float black=clamp((0.02875403f-d.luma)/0.02875403f,0.0f,1.0f);
        float high=s.maximum>0?s.chroma*max(s.luma/s.maximum-0.26129702f,0.0f):0;
        float colored=clamp((0.11519971f-d.luma)/0.11519971f,0.0f,1.0f)*clamp(d.chroma*4.0f,0.0f,1.0f);
        exponent=max(1.0f+black*(0.20662805f*contrast*s.luma+0.11479953f*chroma_scale*s.chroma*s.maximum+0.4492354f*chroma_scale*high)
            +0.48728964f*max(chroma_scale-0.9483659f,0.0f)*colored*s.chroma*channel+suppression,0.03f);
    }
    float compensated=pow(float(coverage)*(1.0f/255.0f),exponent);
    uint corrected=axis_coverage(compensated,s,d,abs(s.perceptual-d.perceptual),axis_strength,axis_limit);
    if(lcd && s.luma<d.luma) {
        float c=float(corrected)*(1.0f/255.0f),core=c*(1.0f-c)*(2.0f*c-1.0f);
        return coverage_u8(c+0.8f*core);
    }
    return corrected;
}
uint auto_mask_over(uint destination,uint source,uint coverage) {return linear_mask_over(destination,source,auto_coverage(destination,source,coverage,false));}
uint auto_subpixel_over(uint destination,uint source,uint mask,uint clip) {
    uint3 m=byte_channels(mask).rgb;
    uint r=auto_coverage(destination,source,mul255(m.r,clip),true),g=auto_coverage(destination,source,mul255(m.g,clip),true),b=auto_coverage(destination,source,mul255(m.b,clip),true);
    uint corrected=axis_mask(destination,source,r|(g<<8)|(b<<16),1.447765f,0.1682842f);
    return linear_subpixel_over(destination,source,corrected,255);
}
