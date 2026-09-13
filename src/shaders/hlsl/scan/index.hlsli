#ifndef TILEINK_HLSL_SCAN_INDEX_HLSLI_INCLUDED
#define TILEINK_HLSL_SCAN_INDEX_HLSLI_INCLUDED

// The caller supplies the active list and mode; this helper owns no resource bindings.
uint dispatched_index(ByteAddressBuffer indices, uint incremental, uint index, uint base) {
    if (incremental != 0u) return indices.Load((base + index) * 4u);
    return index;
}

#endif // TILEINK_HLSL_SCAN_INDEX_HLSLI_INCLUDED
