#ifndef TILEINK_HLSL_SCAN_CONFIG_HLSLI_INCLUDED
#define TILEINK_HLSL_SCAN_CONFIG_HLSLI_INCLUDED

struct ScanConfig {
    uint clear_len; uint backdrop_len; uint path_count; uint scan_chunk_count;
    uint line_count; uint segment_capacity; uint incremental; uint line_base;
    uint path_base; uint chunk_base; uint backdrop_base;
};

#endif // TILEINK_HLSL_SCAN_CONFIG_HLSLI_INCLUDED
