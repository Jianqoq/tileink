// Independent MSL implementation of the same integer probe/32-byte ABI.
struct ProbeParams {
    uint count;
    uint source_offset;
    uint destination_offset;
    uint stride;
    uint4 value;
};

static_assert(sizeof(ProbeParams) == 32, "ProbeParams size");
static_assert(alignof(ProbeParams) == 16, "ProbeParams alignment");
