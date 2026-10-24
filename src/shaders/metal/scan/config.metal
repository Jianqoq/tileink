struct ScanConfig {
    uint clear_len,backdrop_len,path_count,scan_chunk_count;
    uint line_count,segment_capacity,incremental,line_base;
    uint path_base,chunk_base,backdrop_base;
};
uint scan_index(const device uint* indices,uint incremental,uint index,uint base) {
    return incremental!=0?indices[base+index]:index;
}
