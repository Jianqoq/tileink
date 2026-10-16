#ifndef TILEINK_HLSL_FINE_GROUP_INCLUDED
#define TILEINK_HLSL_FINE_GROUP_INCLUDED
#include "inputs.hlsli"
#include "constants.hlsli"
#include "../constants.hlsli"
#include "../shared/pixel.hlsli"

struct FineGroup {
    uint kind;
    float4 parent_pixel;
    uint parent_clip;
    uint layer_alpha;
    uint payload;
};

uint group_spill_word(FineInputs input, uint tile, uint lane, uint depth) {
    return input.config.group_spill_base +
        ((tile * input.config.group_spill_depth + depth) * FINE_WORKGROUP_SIZE + lane) * FINE_GROUP_SPILL_FIELDS;
}

// Local parents retain float precision. Only spill storage uses the canonical
// five-word RGBA8 representation, matching the production WGSL stack.
bool push_group(FineInputs input, uint tile, uint lane, FineGroup value,
                inout uint depth, inout FineGroup slot0, inout FineGroup slot1) {
    if (depth == 0u) slot0 = value;
    else if (depth == 1u) slot1 = value;
    else {
        uint spill_depth = depth - FINE_LOCAL_GROUP_DEPTH;
        if (spill_depth >= input.config.group_spill_depth) return false;
        uint address = group_spill_word(input, tile, lane, spill_depth) * 4u;
        input.spills.Store4(address, uint4(value.kind, unorm_to_rgba8(value.parent_pixel), value.parent_clip, value.layer_alpha));
        input.spills.Store(address + 16u, value.payload);
    }
    depth += 1u;
    return true;
}

bool pop_group(FineInputs input, uint tile, uint lane, inout uint depth,
               FineGroup slot0, FineGroup slot1, out FineGroup value) {
    value = (FineGroup)0;
    if (depth == 0u) return false;
    depth -= 1u;
    if (depth == 0u) value = slot0;
    else if (depth == 1u) value = slot1;
    else {
        uint spill_depth = depth - FINE_LOCAL_GROUP_DEPTH;
        if (spill_depth >= input.config.group_spill_depth) return false;
        uint address = group_spill_word(input, tile, lane, spill_depth) * 4u;
        uint4 words = input.spills.Load4(address);
        value.kind = words.x;
        value.parent_pixel = rgba8_to_unorm(words.y);
        value.parent_clip = words.z;
        value.layer_alpha = words.w;
        value.payload = input.spills.Load(address + 16u);
    }
    return true;
}
#endif
