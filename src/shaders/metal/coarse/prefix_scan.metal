// Two independent modular-u32 exclusive scans. All lanes, including tail lanes,
// participate in every barrier; callers supply zero for inactive records.
uint2 exclusive_prefix(uint2 value, uint lane, threadgroup uint2* scratch, thread uint2& total) {
    scratch[lane] = value;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for (uint step = 1; step < 256; step *= 2) {
        uint2 previous = lane >= step ? scratch[lane - step] : uint2(0);
        threadgroup_barrier(mem_flags::mem_threadgroup);
        scratch[lane] += previous;
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    uint2 offset = scratch[lane] - value;
    total = scratch[255];
    threadgroup_barrier(mem_flags::mem_threadgroup);
    return offset;
}
