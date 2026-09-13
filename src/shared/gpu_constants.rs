//! Algorithm constants shared by host planning and generated shader preludes.
//! These are algorithm choices, not API alignment requirements or device limits.
pub(crate) const CUMSUM_CHUNK_SIZE: u32 = 256;
const _: () = assert!(CUMSUM_CHUNK_SIZE.is_power_of_two() && CUMSUM_CHUNK_SIZE <= 1024);
