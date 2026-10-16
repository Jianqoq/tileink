#ifndef TILEINK_HLSL_COARSE_CONFIG_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_CONFIG_HLSLI_INCLUDED

struct CoarseConfig {
    uint tile_count;
    uint tiles_width;
    uint tiles_height;
    uint draw_start;
    uint draw_end;
    uint layer_stack_start;
    uint layer_stack_end;
    uint ptcl_capacity;
    uint glyph_capacity;
    uint chunk_count;
    uint text_run_count;
    uint text_glyph_count;
    uint tile_draw_index_count;
    uint emit_chunk_capacity;
    uint paint_brush_base;
    uint text_enabled;
    uint active_tile_count;
    uint active_tile_list_base;
    uint incremental;
};

#endif // TILEINK_HLSL_COARSE_CONFIG_HLSLI_INCLUDED
