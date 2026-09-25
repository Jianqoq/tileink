float4 composite_glyphs(FineInput input,texture2d_array<float> atlas,sampler sampling,constant TextureTable& table,
    float4 pixel,uint start,uint end,FineDraw draw,uint2 xy,uint clip) {
    int2 point=int2(floor(affine_point(draw.inverse,float2(xy)+0.5f)));
    uint list_base=input.config->tile_count*6+input.config->ptcl_capacity*6;
    for(uint i=start;i<end;++i) {
        uint index=input.coarse[list_base+i];
        Words glyph=input.text.offset(index*3);
        if(glyph[0]==0xffffffffu) continue;
        Words image=input.text.offset(input.config->text_image_base+glyph[0]*6);
        int x0=as_type<int>(glyph[1])+as_type<int>(image[0]),y0=as_type<int>(glyph[2])-as_type<int>(image[1]);
        int2 local=point-int2(x0,y0);
        if(any(local<0) || local.x>=int(image[2]) || local.y>=int(image[3])) continue;
        uint data=input.text[input.config->text_image_data_base+image[5]+uint(local.y)*image[2]+uint(local.x)];
        uint kind=image[4];
        if(kind==0 || kind==3) {
            uint alpha=mul255(data,clip);
            if(alpha) {
                uint color=draw_brush(input,atlas,sampling,table,draw,float2(xy)+0.5f);
                pixel=kind==0?over_float(pixel,scale_float(color,alpha)):unpack_pixel(auto_mask_over(pack_pixel(pixel),color,alpha));
            }
        } else if(kind==1) pixel=over_float(pixel,scale_float(data,clip));
        else if(kind==4) pixel=unpack_pixel(auto_mask_over(pack_pixel(pixel),data,clip));
        else if(kind==2 || kind==5) {
            uint color=draw_brush(input,atlas,sampling,table,draw,float2(xy)+0.5f);
            pixel=unpack_pixel(kind==2?subpixel_over(pack_pixel(pixel),color,data,clip):auto_subpixel_over(pack_pixel(pixel),color,data,clip));
        }
    }
    return pixel;
}
