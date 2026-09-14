#ifndef TILEINK_HLSL_COARSE_TAGS_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_TAGS_HLSLI_INCLUDED
#include "../shared/draw_tags.hlsli"

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
