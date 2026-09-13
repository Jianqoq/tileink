//! Vulkan descriptor/image limits must be checked before void recording calls;
//! successful allocation does not establish that a descriptor range is legal.
use super::{Dispatch, Result};

pub(super) fn validate(command: &Dispatch, buffer_bytes: u32, image_width: u32) -> Result<()> {
    if command.source.len() as u64 > buffer_bytes as u64
        || command.destination.len() as u64 > buffer_bytes as u64
        || (command.entry == "sample_words" && command.params.value[2] > image_width)
    {
        return Err("native Vulkan resource exceeds physical device limits".into());
    }
    Ok(())
}

#[test]
fn descriptor_range_and_texture_extent_cannot_exceed_device_limits() {
    let mut command = Dispatch {
        entry: "copy_words",
        params: super::super::program::Params {
            count: 1,
            source_offset: 0,
            destination_offset: 0,
            stride: 4,
            value: [0, 0, 1, 0],
        },
        source: vec![0; 4],
        destination: vec![0; 4],
    };
    assert!(validate(&command, 4, 1).is_ok());
    assert!(validate(&command, 3, 1).is_err());
    command.source.resize(8, 0);
    assert!(validate(&command, 4, 1).is_err());
    command.source.resize(4, 0);
    command.destination.resize(8, 0);
    assert!(validate(&command, 4, 1).is_err());
    command.destination.resize(4, 0);
    command.entry = "sample_words";
    assert!(validate(&command, 4, 0).is_err());
    assert!(validate(&command, 4, 1).is_ok());
}
