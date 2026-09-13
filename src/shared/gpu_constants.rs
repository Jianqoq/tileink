//! Host view of the canonical HLSL constants; no values are defined here.
include!(concat!(env!("OUT_DIR"), "/tileink_gpu_constants.rs"));

const _: () = assert!(CUMSUM_CHUNK_SIZE.is_power_of_two() && CUMSUM_CHUNK_SIZE <= 1024);
const _: () = assert!(SCAN_CHUNK_SIZE.is_power_of_two() && SCAN_CHUNK_SIZE <= 1024);
const _: () = assert!(RANGE_SCATTER_WORKGROUP_SIZE > 0 && RANGE_SCATTER_WORKGROUP_SIZE <= 1024);
const _: () = assert!(COARSE_WORKGROUP_SIZE.is_power_of_two() && COARSE_WORKGROUP_SIZE <= 1024);
const _: () = assert!(FILTER_WORKGROUP_SIZE > 0 && FILTER_WORKGROUP_SIZE <= 1024);
