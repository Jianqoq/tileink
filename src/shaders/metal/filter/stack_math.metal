// Offscreen stack ranges contain only clips; groups own separate surfaces.
uint composite_clip_stack(constant FilterConfig& config,LayerGeometry geometry,Words layers,uint destination,uint source,uint mask,
    uint2 xy,bool use_mask,bool force_blend) {
    uint clip=255;
    for(uint index=config.layer_stack_start;index<config.layer_stack_end;++index) {
        uint alpha=layer_alpha(geometry,layers[index*3+1],config.paint_sdf_shadow_base,xy,uint2(config.tiles_width,config.tiles_height));
        clip=mul255(clip,alpha);
    }
    uint scaled=scale_pixel(source,use_mask?mul255(clip,mask>>24):clip);
    if(force_blend) return scaled>>24?blend_pixel(destination,scaled,config.blend_mode):destination;
    return source_over(destination,scaled);
}
