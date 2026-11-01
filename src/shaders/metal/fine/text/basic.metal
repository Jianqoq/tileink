// RGB coverage cannot be stored as one alpha against an unknown backdrop.
// Preserve its mean coverage on transparent layers instead of thickening it
// with the maximum channel; opaque destinations retain LCD rendering.
uint3 text_subpixel_coverage(uint destination,uint mask,uint clip) {
    uint3 bytes=byte_channels(mask).rgb;
    uint3 coverage=uint3(mul255(bytes.r,clip),mul255(bytes.g,clip),mul255(bytes.b,clip));
    if((destination>>24)!=255) {
        uint gray=(coverage.r+coverage.g+coverage.b+1)/3;
        coverage=uint3(gray);
    }
    return coverage;
}
uint subpixel_over(uint destination,uint source,uint mask,uint clip) {
    uint sa=source>>24;
    if(!sa || !clip) return destination;
    uint3 m=text_subpixel_coverage(destination,mask,clip);
    if(all(m==0)) return destination;
    uint3 coverage=uint3(mul255(sa,m.r),mul255(sa,m.g),mul255(sa,m.b));
    uint alpha=max(max(coverage.r,coverage.g),coverage.b);
    uint4 s=byte_channels(source),d=byte_channels(destination);
    return pack_bytes(uint4(mul255(s.r,m.r)+mul255(d.r,255-coverage.r),mul255(s.g,m.g)+mul255(d.g,255-coverage.g),
        mul255(s.b,m.b)+mul255(d.b,255-coverage.b),alpha+mul255(d.a,255-alpha)));
}
uint linear_mask_over(uint destination,uint source,uint coverage) {
    if(!(source>>24) || !coverage) return destination;
    float c=float(coverage)*(1.0f/255.0f),sa=float(source>>24)*(1.0f/255.0f),da=float(destination>>24)*(1.0f/255.0f);
    float3 s=linear_premul(source,sa),d=linear_premul(destination,da);
    float alpha=sa*c;
    return pack_linear(s*c+d*(1.0f-alpha),alpha+da*(1.0f-alpha));
}
uint linear_subpixel_over(uint destination,uint source,uint mask,uint clip) {
    if(!(source>>24) || !clip) return destination;
    float3 m=float3(text_subpixel_coverage(destination,mask,clip))*(1.0f/255.0f);
    if(all(m==0)) return destination;
    float sa=float(source>>24)*(1.0f/255.0f),da=float(destination>>24)*(1.0f/255.0f);
    float3 s=linear_premul(source,sa),d=linear_premul(destination,da),coverage=sa*m;
    float alpha=max(max(coverage.r,coverage.g),coverage.b);
    return pack_linear(s*m+d*(1.0f-coverage),alpha+da*(1.0f-alpha));
}

uint subpixel_over_f32(uint destination,uint source,float3 coverage) {
    if(!(source>>24) || all(coverage==0)) return destination;
    if((destination>>24)!=255) coverage=float3((coverage.r+coverage.g+coverage.b)/3.0f);
    float3 amount=float(source>>24)*(1.0f/255.0f)*coverage;
    float3 output=float3(byte_channels(source).rgb)*coverage+float3(byte_channels(destination).rgb)*(1.0f-amount);
    float maximum=max(max(amount.r,amount.g),amount.b);
    float alpha=maximum*255.0f+float(destination>>24)*(1.0f-maximum);
    return pack_bytes(uint4(uint3(output+0.5f),uint(alpha+0.5f)));
}
