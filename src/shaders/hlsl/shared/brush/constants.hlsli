#ifndef TILEINK_HLSL_BRUSH_CONSTANTS_HLSLI_INCLUDED
#define TILEINK_HLSL_BRUSH_CONSTANTS_HLSLI_INCLUDED
static const uint BRUSH_HEADER_WORDS=9u;
static const uint BRUSH_PARAM_WORDS=12u;
static const uint BRUSH_SOLID=1u;
static const uint BRUSH_LINEAR=2u;
static const uint BRUSH_RADIAL=3u;
static const uint BRUSH_SWEEP=4u;
static const uint BRUSH_FOUR_CORNER=5u;
static const uint BRUSH_PATTERN=6u;
static const uint BRUSH_PATTERN_RESOURCE=7u;
static const uint BRUSH_COLOR_WORD=4u;
static const uint BRUSH_IMAGE_ALPHA_WORD=7u;
static const uint BRUSH_COLOR_OFFSET=BRUSH_COLOR_WORD*4u;
static const uint BRUSH_IMAGE_ALPHA_OFFSET=BRUSH_IMAGE_ALPHA_WORD*4u;
static const uint BRUSH_EXTEND_REPEAT=1u;
static const uint BRUSH_EXTEND_REFLECT=2u;
#endif // TILEINK_HLSL_BRUSH_CONSTANTS_HLSLI_INCLUDED
