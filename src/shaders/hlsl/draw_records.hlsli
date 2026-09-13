#ifndef TILEINK_HLSL_DRAW_RECORDS_HLSLI_INCLUDED
#define TILEINK_HLSL_DRAW_RECORDS_HLSLI_INCLUDED

// Raw scene record offsets are checked against Rust's repr(C) records.
static const uint DRAW_RECORD_STRIDE = 124u;
static const uint DRAW_PATH = 0u;
static const uint DRAW_GLYPH_RUN = 4u;
static const uint DRAW_SDF = 8u;
static const uint DRAW_SDF_LEN = 12u;
static const uint DRAW_SHADOW = 16u;
static const uint DRAW_BRUSH_OFFSET = 24u;
static const uint DRAW_SOLID_RECT = 72u;
static const uint DRAW_TAG = 32u;
static const uint DRAW_FILL_RULE = 36u;
static const uint DRAW_PIXEL_BOUNDS = 40u;
static const uint DRAW_TRANSFORM = 76u;
static const uint LAYER_RECORD_STRIDE = 12u;
static const uint GLYPH_RUN_STRIDE = 8u;
static const uint GLYPH_RECORD_STRIDE = 12u;
static const uint GLYPH_IMAGE_STRIDE = 24u;

#endif // TILEINK_HLSL_DRAW_RECORDS_HLSLI_INCLUDED
