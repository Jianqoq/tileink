const SHARED_BLUR_TILE_WIDTH: u32 = 16u;
const SHARED_BLUR_TILE_HEIGHT: u32 = 16u;
const SHARED_BLUR_MAX_RADIUS: u32 = 16u;
const SHARED_BLUR_WORKGROUP_SIZE: u32 = SHARED_BLUR_TILE_WIDTH * SHARED_BLUR_TILE_HEIGHT;
var<workgroup> shared_blur_pixels: array<u32, 768>;

@compute @workgroup_size(256)
fn filter_clear_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    target_store_ix(target_ix_for_region_ix(region_ix), config.clear_color);
}

@compute @workgroup_size(256)
fn filter_copy_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, source_pixel_ix(ix));
}

@compute @workgroup_size(256)
fn filter_source_alpha_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, source_pixel_ix(ix) & 0xff000000u);
}

@compute @workgroup_size(256)
fn filter_source_over_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let ix = target_ix_for_region_ix(region_ix);
    target_store_ix(ix, blend_premul_u8(target_load_ix(ix), source_pixel_ix(ix), 3u << 8u));
}

@compute @workgroup_size(256)
fn filter_tile_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let source_x0 = u32(config.rect_x0);
    let source_y0 = u32(config.rect_y0);
    let source_width = u32(config.rect_x1) - source_x0;
    let source_height = u32(config.rect_y1) - source_y0;
    if (source_width == 0u || source_height == 0u) {
        return;
    }
    let sx = source_x0 + (xy.x + source_width - (source_x0 % source_width)) % source_width;
    let sy = source_y0 + (xy.y + source_height - (source_y0 % source_height)) % source_height;
    target_store_at(xy.x, xy.y, source_pixel_at(sx, sy));
}

@compute @workgroup_size(256)
fn filter_offset_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    let sx = i32(xy.x) - config.offset_x;
    let sy = i32(xy.y) - config.offset_y;
    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    var pixel = 0u;
    if (
        sx >= i32(config.region_x0) &&
        sx < region_x1 &&
        sy >= i32(config.region_y0) &&
        sy < region_y1
    ) {
        pixel = source_pixel_at(u32(sx), u32(sy));
    }
    target_store_at(xy.x, xy.y, pixel);
}

@compute @workgroup_size(256)
fn filter_turbulence_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    target_store_at(xy.x, xy.y, filter_turbulence_pixel(f32(xy.x), f32(xy.y)));
}

@compute @workgroup_size(256)
fn filter_flood_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }
    let xy = xy_for_region_ix(region_ix);
    target_store_at(xy.x, xy.y, sample_brush(config.brush_offset, f32(xy.x) + 0.5, f32(xy.y) + 0.5));
}

@compute @workgroup_size(256)
fn filter_drop_shadow_mask_region(@builtin(global_invocation_id) gid: vec3<u32>) {
    let region_ix = gid.x;
    if (region_ix >= config.pixel_count) {
        return;
    }

    let xy = xy_for_region_ix(region_ix);
    let alpha = source_pixel_at(xy.x, xy.y) >> 24u;
    if (alpha == 0u) {
        return;
    }

    let tx = i32(xy.x) + config.offset_x;
    let ty = i32(xy.y) + config.offset_y;
    let region_x1 = i32(config.region_x0 + config.region_width);
    let region_y1 = i32(config.region_y0 + config.region_height);
    if (
        tx >= i32(config.region_x0) &&
        tx < region_x1 &&
        ty >= i32(config.region_y0) &&
        ty < region_y1
    ) {
        target_store_at(u32(tx), u32(ty), gray_alpha(alpha));
    }
}

@compute @workgroup_size(256)
