uint subpixel_over(uint destination,uint source,uint mask,uint clip) {
    uint sa=source>>24;
    if(!sa || !clip) return destination;
    uint3 m=byte_channels(mask).rgb;
    m=uint3(mul255(m.r,clip),mul255(m.g,clip),mul255(m.b,clip));
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
    uint3 bytes=byte_channels(mask).rgb;
    float3 m=float3(mul255(bytes.r,clip),mul255(bytes.g,clip),mul255(bytes.b,clip))*(1.0f/255.0f);
    if(all(m==0)) return destination;
    float sa=float(source>>24)*(1.0f/255.0f),da=float(destination>>24)*(1.0f/255.0f);
    float3 s=linear_premul(source,sa),d=linear_premul(destination,da),coverage=sa*m;
    float alpha=max(max(coverage.r,coverage.g),coverage.b);
    return pack_linear(s*m+d*(1.0f-coverage),alpha+da*(1.0f-alpha));
}
