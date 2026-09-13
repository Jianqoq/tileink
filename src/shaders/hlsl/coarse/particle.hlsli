#ifndef TILEINK_HLSL_COARSE_PARTICLE_HLSLI_INCLUDED
#define TILEINK_HLSL_COARSE_PARTICLE_HLSLI_INCLUDED

#include "../coarse_records.hlsli"
#include "config.hlsli"
#include "tags.hlsli"

static const uint PTCL_END = 0u;
static const uint PTCL_FILL = 1u;
static const uint PTCL_COLOR = 2u;
static const uint PTCL_BEGIN_CLIP = 3u;
static const uint PTCL_END_CLIP = 4u;
static const uint PTCL_BEGIN_OPACITY = 5u;
static const uint PTCL_END_OPACITY = 6u;
static const uint PTCL_BEGIN_BLEND = 7u;
static const uint PTCL_END_BLEND = 8u;
static const uint PTCL_SDF = 9u;
static const uint PTCL_GLYPH = 10u;
static const uint PTCL_PATH_GLYPH = 11u;
static const uint PTCL_BEGIN_SDF_CLIP = 12u;
static const uint PTCL_IMAGE = 13u;

struct Particle {
    bool valid;
    uint glyph_count, tag, winding, fill_rule;
    uint2 segments;
    uint color;
};
Particle empty_particle() {
    Particle result;
    result.valid = false;
    result.glyph_count = 0u;
    result.tag = PTCL_FILL;
    result.winding = 0u;
    result.fill_rule = 0u;
    result.segments = uint2(0u,0u);
    result.color = 0u;
    return result;
}
void store_particle(RWByteAddressBuffer work, ConstantBuffer<CoarseConfig> settings,
    uint destination, uint tag, uint winding, uint fill_rule, uint2 segments, uint color) {
    if (destination >= settings.ptcl_capacity) return;
    uint base = settings.tile_count * COARSE_TILE_RECORD_STRIDE + destination * COARSE_PTCL_RECORD_STRIDE;
    work.Store4(base, uint4(tag, winding, fill_rule, segments.x));
    work.Store2(base + 16u, uint2(segments.y, color));
}
void store_particle_value(RWByteAddressBuffer work, ConstantBuffer<CoarseConfig> settings, uint destination, Particle value) {
    store_particle(work, settings, destination, value.tag, value.winding, value.fill_rule, value.segments, value.color);
}


#endif // TILEINK_HLSL_COARSE_PARTICLE_HLSLI_INCLUDED
