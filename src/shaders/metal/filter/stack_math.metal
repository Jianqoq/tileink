struct CompositeGroup {uint kind,parent,clip,alpha,payload;};
uint composite_stack(constant FilterConfig& config,LayerGeometry geometry,Words layers,uint destination,uint source,uint mask,
    uint2 xy,bool use_mask,bool force_blend) {
    CompositeGroup groups[64];
    uint depth=0,clip=255,pixel=destination;
    for(uint index=config.layer_stack_start;index<config.layer_stack_end;++index) {
        Words layer=layers.offset(index*3);
        uint kind=layer[0],alpha=layer_alpha(geometry,layer[1],config.paint_sdf_shadow_base,xy,uint2(config.tiles_width,config.tiles_height));
        if(!kind) clip=mul255(clip,alpha);
        else if((kind==1 || kind==2) && depth<64) {groups[depth++]={kind,pixel,clip,alpha,layer[2]};pixel=0;}
    }
    uint scaled=scale_pixel(source,use_mask?mul255(clip,mask>>24):clip);
    if(force_blend) {if(scaled>>24) pixel=blend_pixel(pixel,scaled,config.blend_mode);}
    else pixel=source_over(pixel,scaled);
    while(depth) {
        CompositeGroup group=groups[--depth];
        uint alpha=mul255(group.alpha,group.clip);
        if(group.kind==1) pixel=source_over(group.parent,scale_pixel(pixel,mul255(alpha,group.payload)));
        else {
            uint foreground=scale_pixel(pixel,alpha);
            pixel=foreground>>24?blend_pixel(group.parent,foreground,group.payload):group.parent;
        }
    }
    return pixel;
}
