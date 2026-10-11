// Coarse proves the emitted stream contains only colors, full-tile images and
// supported solid SDFs, without a clip/group stack. Reused slots are classified
// again before fine executes. Preserve the general rasterizer's blend order.
float4 simple_tile_pixel(FineInput input,texture2d_array<float> atlas,sampler sampling,
    constant TextureTable& table,float4 pixel,uint tile,uint2 xy) {
    uint start=fine_tile_start(input,tile),end=input.coarse[tile*6+2];
    float2 position=float2(xy)+0.5f;
    for(uint i=start;i<end;++i) {
        Particle p=particle_at(input,i);
        if(!p.tag) break;
        uint alpha=255,color=p.color;
        if(p.tag!=2) {
            FineDraw draw=fine_draw(input.draws,p.color);
            if(p.tag==9) alpha=coverage_u8(draw_coverage(input,draw,position));
            if(!alpha) continue;
            color=draw_brush(input,atlas,sampling,table,draw,position);
        }
        pixel=alpha==255 && (color>>24)==255?unpack_pixel(color):over_float(pixel,scale_float(color,alpha));
    }
    return pixel;
}

float4 shade_tile_pixel(FineInput input,texture2d_array<float> atlas,sampler sampling,
    constant TextureTable& table,float4 initial,uint tile,uint lane,uint2 xy) {
    uint kind=input.coarse[input.config->fine_tile_kind_base+tile] & TILE_KIND_MASK;
    if(kind==TILE_KIND_INTERPRETER) return fine_pixel(input,atlas,sampling,table,initial,tile,lane,xy);
    if(kind==TILE_KIND_EMPTY) return initial;
    return simple_tile_pixel(input,atlas,sampling,table,initial,tile,xy);
}
