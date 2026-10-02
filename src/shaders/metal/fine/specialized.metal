// Coarse classification proves these tiles need no clip/group stack. Keep the
// same per-particle rounding and blend order as the general interpreter. If an
// unexpected tag appears, discard partial output and interpret the tile once
// from its original destination; never blend an already-applied prefix twice.
bool simple_tile_pixel(FineInput input,texture2d_array<float> atlas,sampler sampling,
    constant TextureTable& table,uint kind,uint tile,uint2 xy,thread float4& pixel) {
    uint start=input.coarse[tile*6+1],end=input.coarse[tile*6+2];
    float2 position=float2(xy)+0.5f;
    for(uint i=start;i<end;++i) {
        Particle p=particle_at(input,i);
        if(!p.tag) break;
        uint alpha=255,color=p.color;
        if(p.tag!=2) {
            if(kind==2 || (p.tag!=9 && p.tag!=13)) return false;
            FineDraw draw=fine_draw(input.draws,p.color);
            if(p.tag==9) alpha=coverage_u8(draw_coverage(input,draw,position));
            if(!alpha) continue;
            color=draw_brush(input,atlas,sampling,table,draw,position);
        }
        pixel=alpha==255 && (color>>24)==255?unpack_pixel(color):over_float(pixel,scale_float(color,alpha));
    }
    return true;
}

float4 shade_tile_pixel(FineInput input,texture2d_array<float> atlas,sampler sampling,
    constant TextureTable& table,float4 initial,uint tile,uint lane,uint2 xy) {
    uint kind=input.coarse[input.config->fine_tile_kind_base+tile];
    if(kind==1) return initial;
    float4 pixel=initial;
    if(kind>=2 && kind<=4 && simple_tile_pixel(input,atlas,sampling,table,kind,tile,xy,pixel)) return pixel;
    return fine_pixel(input,atlas,sampling,table,initial,tile,lane,xy);
}
