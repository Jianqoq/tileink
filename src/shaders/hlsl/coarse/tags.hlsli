#ifndef TILEINK_HLSL_COARSE_TAGS_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_TAGS_HLSLI_INCLUDED

static const uint INVALID_INDEX = 4294967295u;
static const uint DRAW_BRUSH = 0u;
static const uint DRAW_CLIP = 1u;
static const uint DRAW_PATH_GLYPH = 5u;
static const uint LAYER_CLIP = 0u;
static const uint LAYER_OPACITY = 1u;
static const uint LAYER_BLEND = 2u;
static const uint FILL_EVEN_ODD = 1u;
static const uint SDF_RECT = 1u;
static const uint TILE_KIND_INTERPRETER = 0u;
static const uint TILE_KIND_EMPTY = 1u;
static const uint TILE_KIND_COLOR = 2u;
static const uint TILE_KIND_SDF = 3u;
static const uint TILE_KIND_MIXED = 4u;
static const uint CHUNK_CLASS_COLOR = 1u;
static const uint CHUNK_CLASS_SDF = 2u;
static const uint CHUNK_CLASS_OTHER = 4u;
static const uint DRAW_FLAT_FLAG = 2147483648u;
static const uint DRAW_FLAT_MASK = 2147483647u;
static const uint COARSE_BIN_SIDE = 16u;

#endif // TILEINK_HLSL_COARSE_TAGS_HLSLI_INCLUDED
