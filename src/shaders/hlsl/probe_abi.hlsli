// Explicit 32-byte constant layout shared with the independent Metal probe.
struct ProbeParams {
    uint count;
    uint source_offset;
    uint destination_offset;
    uint stride;
    uint4 value;
};
RWByteAddressBuffer destination : register(u0, space0);
ByteAddressBuffer source : register(t1, space0);
ConstantBuffer<ProbeParams> params : register(b2, space0);
