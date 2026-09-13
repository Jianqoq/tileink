//! Algorithm constants shared by host planning and generated shader preludes.
//! These are algorithm choices, not API alignment requirements or device limits.
pub(crate) const CUMSUM_CHUNK_SIZE: u32 = 256;
const _: () = assert!(CUMSUM_CHUNK_SIZE.is_power_of_two() && CUMSUM_CHUNK_SIZE <= 1024);

pub(crate) const SCAN_CHUNK_SIZE: u32 = 256;
const _: () = assert!(SCAN_CHUNK_SIZE.is_power_of_two() && SCAN_CHUNK_SIZE <= 1024);

/// Physical tile dimension shared by host geometry and shader generation.
pub const TILE_SIZE: u32 = 16;

/// Threads cooperatively copying one uploaded range.
pub(crate) const RANGE_SCATTER_WORKGROUP_SIZE: u32 = 256;
const _: () = assert!(RANGE_SCATTER_WORKGROUP_SIZE > 0 && RANGE_SCATTER_WORKGROUP_SIZE <= 1024);

/// Lanes in the coarse allocation scan and cooperative tile interpreter.
pub(crate) const COARSE_WORKGROUP_SIZE: u32 = 256;
const _: () = assert!(COARSE_WORKGROUP_SIZE.is_power_of_two() && COARSE_WORKGROUP_SIZE <= 1024);
