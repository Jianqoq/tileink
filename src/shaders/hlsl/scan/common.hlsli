#include "../dispatch.hlsli"
struct ScanConfig {
    uint clear_len; uint backdrop_len; uint path_count; uint scan_chunk_count;
    uint line_count; uint segment_capacity; uint incremental; uint line_base;
    uint path_base; uint chunk_base; uint backdrop_base;
};
ConstantBuffer<ScanConfig> config:register(b0,space0);
// Each entry declares its active-index buffer before including this shared code.
uint dispatched_index(uint index,uint base) {
    if (config.incremental != 0u) return active_indices.Load((base+index)*4u);
    return index;
}
