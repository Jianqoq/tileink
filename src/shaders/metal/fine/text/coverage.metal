// One correcting coverage transfer before encoded premultiplied source-over;
// transparent and opaque destinations therefore retain consistent text weight.
float3 text_foreground(uint source) {
    float alpha=float(source>>24);
    return alpha>0?clamp(float3(byte_channels(source).rgb)/alpha,0.0f,1.0f):float3(0);
}
float text_correcting_coverage(uint coverage,float foreground) {
    if(!coverage || coverage==255) return float(coverage)*(1.0f/255.0f);
    float raw=float(coverage)*(1.0f/255.0f),background=1.0f-foreground;
    float bg=srgb_linear(background),enhanced=raw+raw*(1.0f-raw)*bg,difference=foreground-background;
    if(abs(difference)<(1.0f/256.0f)) return enhanced;
    float output=linear_srgb(mix(bg,srgb_linear(foreground),enhanced));
    return clamp((output-background)/difference,0.0f,1.0f);
}
uint auto_mask_over(uint destination,uint source,uint coverage) {
    if(!(source>>24) || !coverage) return destination;
    return subpixel_over_f32(destination,source,float3(text_correcting_coverage(coverage,text_luma(text_foreground(source)))));
}
uint auto_subpixel_over(uint destination,uint source,uint mask,uint clip) {
    if(!(source>>24) || !clip) return destination;
    uint3 m=byte_channels(mask).rgb;float3 f=text_foreground(source);
    float3 corrected=float3(text_correcting_coverage(mul255(m.r,clip),f.r),text_correcting_coverage(mul255(m.g,clip),f.g),text_correcting_coverage(mul255(m.b,clip),f.b));
    return subpixel_over_f32(destination,source,corrected);
}
