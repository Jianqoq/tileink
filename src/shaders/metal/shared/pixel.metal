// Stored premultiplied RGBA8 is the quantization boundary, including alpha-zero RGB.
uint4 byte_channels(uint pixel) { return uint4(pixel,pixel>>8,pixel>>16,pixel>>24)&255u; }
uint pack_bytes(uint4 value) { return value.x|(value.y<<8)|(value.z<<16)|(value.w<<24); }
float4 unpack_pixel(uint pixel) { return float4(byte_channels(pixel))*(1.0f/255.0f); }
uint pack_pixel(float4 value) { return pack_bytes(uint4(clamp(value,0.0f,1.0f)*255.0f+0.5f)); }
uint source_over(uint destination,uint source) {
    uint4 foreground=byte_channels(source), background=byte_channels(destination);
    if (foreground.a==0) return destination;
    uint4 product=background*(255-foreground.a)+128;
    return pack_bytes(foreground+((product+(product>>8))>>8));
}
uint coverage_u8(float coverage) { return uint(clamp(coverage,0.0f,1.0f)*255.0f+0.5f); }
uint mul255(uint a, uint b) { uint p=a*b+128;return (p+(p>>8))>>8; }
uint scale_pixel(uint source, uint factor) {
    if (!factor) return 0;
    if (factor == 255) return source;
    uint4 p=byte_channels(source)*factor+128;
    return pack_bytes((p+(p>>8))>>8);
}
float4 scale_float(uint source, uint factor) {
    if (!factor || !(source>>24)) return float4(0);
    return float4(byte_channels(source))*(float(factor)*(1.0f/65025.0f));
}
float4 over_float(float4 destination, float4 source) {
    if (source.a <= 0) return destination;
    if (source.a >= 1) return source;
    return source+destination*(1.0f-source.a);
}
uint mix_pixel(uint a, uint b, float t) {
    float4 left=float4(byte_channels(a)), right=float4(byte_channels(b));
    return pack_bytes(uint4(clamp(fma(right-left,t,left)+0.5f,0.0f,255.0f)));
}
