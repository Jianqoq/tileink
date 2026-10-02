float4 fine_pixel(FineInput input,texture2d_array<float> atlas,sampler sampling,constant TextureTable& table,
    float4 pixel,uint tile,uint lane,uint2 xy) {
    PixelStack stack={255,0,0,{255,255,255,255},{}};
    uint start=input.coarse[tile*6+1],end=input.coarse[tile*6+2];
    float2 position=float2(xy)+0.5f;
    for(uint i=start;i<end;++i) {
        Particle p=particle_at(input,i);
        if(!p.tag) break;
        if(p.tag==2 || p.tag==9 || p.tag==13) {
            if(!stack.mask) continue;
            uint alpha=stack.mask,color=p.color;
            if(p.tag!=2) {
                FineDraw draw=fine_draw(input.draws,p.color);
                if(p.tag==9) alpha=mul255(coverage_u8(draw_coverage(input,draw,position)),stack.mask);
                if(!alpha) continue;
                color=draw_brush(input,atlas,sampling,table,draw,position);
            }
            pixel=alpha==255 && (color>>24)==255?unpack_pixel(color):over_float(pixel,scale_float(color,alpha));
        } else if(p.tag==10) {
            FineDraw draw=fine_draw(input.draws,p.color);
            if(stack.mask && all(int2(xy)>=draw.bounds.xy) && all(int2(xy)<draw.bounds.zw))
                pixel=composite_glyphs(input,atlas,sampling,table,pixel,p.start,p.end,draw,xy,stack.mask);
        } else if(p.tag==4) pop_clip(input,tile,lane,stack);
        else if(p.tag==12) {
            uint alpha=stack.mask?coverage_u8(draw_coverage(input,fine_draw(input.draws,p.color),position)):0;
            push_clip(input,tile,lane,stack);
            stack.mask=mul255(stack.mask,alpha);
        } else if(p.tag==6 || p.tag==8) {
            Group group;
            if(pop_group(input,tile,lane,stack,group)) {
                if(group.kind==5) {
                    uint alpha=mul255(mul255(group.alpha,group.clip),group.payload);
                    pixel=over_float(group.parent,scale_float(pack_pixel(pixel),alpha));
                } else if(group.kind==7) {
                    uint source=scale_pixel(pack_pixel(pixel),mul255(group.alpha,group.clip));
                    pixel=(source>>24)==0?group.parent:unpack_pixel(blend_pixel(pack_pixel(group.parent),source,group.payload));
                }
            }
        } else if(p.tag==1 || p.tag==11 || p.tag==3 || p.tag==5 || p.tag==7) {
            uint alpha=fill_alpha(input.segments,p.backdrop,p.rule,p.start,p.end,uint2(lane%16,lane/16));
            if(p.tag==3) {push_clip(input,tile,lane,stack);stack.mask=mul255(stack.mask,alpha);}
            else if(p.tag==5 || p.tag==7) {
                if(push_group(input,tile,lane,stack,Group{p.tag,pixel,stack.mask,alpha,p.color})) pixel=float4(0);
            } else {
                alpha=mul255(alpha,stack.mask);
                if(alpha) {
                    uint color=draw_brush(input,atlas,sampling,table,fine_draw(input.draws,p.color),position);
                    if(p.tag==11) pixel=unpack_pixel(auto_mask_over(pack_pixel(pixel),color,alpha));
                    else pixel=alpha==255 && (color>>24)==255?unpack_pixel(color):over_float(pixel,scale_float(color,alpha));
                }
            }
        }
    }
    return pixel;
}
