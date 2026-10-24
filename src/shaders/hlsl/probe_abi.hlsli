#ifndef TILEINK_HLSL_PROBE_ABI_HLSLI_INCLUDED
#define TILEINK_HLSL_PROBE_ABI_HLSLI_INCLUDED

// Explicit 32-byte constant layout shared with the independent Metal probe.
struct ProbeParams {
    uint count;
    uint source_offset;
    uint destination_offset;
    uint stride;
    uint4 value;
};

#endif // TILEINK_HLSL_PROBE_ABI_HLSLI_INCLUDED
