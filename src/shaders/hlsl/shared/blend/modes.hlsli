#ifndef TILEINK_HLSL_BLEND_MODES_HLSLI_INCLUDED
#define TILEINK_HLSL_BLEND_MODES_HLSLI_INCLUDED
// Serialized scene blend-mode byte values, shared by fine and effects.
static const uint MIX_NORMAL=0u;
static const uint MIX_MULTIPLY=1u;
static const uint MIX_SCREEN=2u;
static const uint MIX_OVERLAY=3u;
static const uint MIX_DARKEN=4u;
static const uint MIX_LIGHTEN=5u;
static const uint MIX_COLOR_DODGE=6u;
static const uint MIX_COLOR_BURN=7u;
static const uint MIX_HARD_LIGHT=8u;
static const uint MIX_SOFT_LIGHT=9u;
static const uint MIX_DIFFERENCE=10u;
static const uint MIX_EXCLUSION=11u;
static const uint MIX_HUE=12u;
static const uint MIX_SATURATION=13u;
static const uint MIX_COLOR=14u;
static const uint MIX_LUMINOSITY=15u;
static const uint COMPOSE_CLEAR=0u;
static const uint COMPOSE_COPY=1u;
static const uint COMPOSE_DEST=2u;
static const uint COMPOSE_SRC_OVER=3u;
static const uint COMPOSE_DEST_OVER=4u;
static const uint COMPOSE_SRC_IN=5u;
static const uint COMPOSE_DEST_IN=6u;
static const uint COMPOSE_SRC_OUT=7u;
static const uint COMPOSE_DEST_OUT=8u;
static const uint COMPOSE_SRC_ATOP=9u;
static const uint COMPOSE_DEST_ATOP=10u;
static const uint COMPOSE_XOR=11u;
static const uint COMPOSE_PLUS=12u;
static const uint COMPOSE_PLUS_LIGHTER=13u;
#endif // TILEINK_HLSL_BLEND_MODES_HLSLI_INCLUDED
