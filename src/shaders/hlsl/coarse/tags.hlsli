#ifndef TILEINK_HLSL_COARSE_TAGS_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_TAGS_HLSLI_INCLUDED
#include "../shared/draw_tags.hlsli"

static const uint SDF_RECT = 1u;
#include "../shared/tile_kinds.hlsli"
static const uint CHUNK_CLASS_COLOR = 1u;
static const uint CHUNK_CLASS_SDF = 2u;
static const uint CHUNK_CLASS_OTHER = 4u;
static const uint DRAW_FLAT_FLAG = 2147483648u;
static const uint DRAW_FLAT_MASK = 2147483647u;
static const uint COARSE_BIN_SIDE = 16u;

#endif // TILEINK_HLSL_COARSE_TAGS_HLSLI_INCLUDED
