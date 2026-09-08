use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use ash::vk;

pub(super) fn query_surface_formats(
    capacity_hint: &AtomicU32,
    mut query: impl FnMut(&mut u32, Option<&mut [vk::SurfaceFormatKHR]>) -> vk::Result,
) -> Result<Vec<vk::SurfaceFormatKHR>, vk::Result> {
    // Querying the count can repeat the driver's full format discovery and heap
    // contention during resize. Remember only capacity: every result is freshly
    // enumerated, and a growing list retries the standard count/data sequence.
    let mut count = capacity_hint.load(Ordering::Relaxed);
    loop {
        if count == 0 {
            query(&mut count, None).result()?;
        }
        let mut formats = alloc::vec![vk::SurfaceFormatKHR::default(); count as usize];
        match query(&mut count, Some(&mut formats)) {
            vk::Result::SUCCESS => {
                formats.truncate(count as usize);
                capacity_hint.store(count, Ordering::Relaxed);
                return Ok(formats);
            }
            vk::Result::INCOMPLETE => count = 0,
            error => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_queries_read_fresh_formats_without_recounting() {
        let hint = AtomicU32::new(0);
        for format in [vk::Format::B8G8R8A8_UNORM, vk::Format::R16G16B16A16_SFLOAT] {
            let warm = hint.load(Ordering::Relaxed) != 0;
            let mut count_queries = 0;
            let mut data_queries = 0;
            let formats = query_surface_formats(&hint, |count, output| {
                if let Some(output) = output {
                    data_queries += 1;
                    output[0] = vk::SurfaceFormatKHR {
                        format,
                        color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
                    };
                } else {
                    count_queries += 1;
                }
                *count = 1;
                vk::Result::SUCCESS
            })
            .unwrap();
            assert_eq!(formats.len(), 1);
            assert_eq!(formats[0].format, format);
            assert_eq!(data_queries, 1);
            assert_eq!(count_queries, u32::from(!warm));
        }
    }

    #[test]
    fn growing_formats_retry_even_when_the_list_changes_after_recounting() {
        let hint = AtomicU32::new(1);
        let mut calls = 0;
        let formats = query_surface_formats(&hint, |count, output| {
            calls += 1;
            match calls {
                1 | 3 => {
                    assert_eq!(output.unwrap().len(), *count as usize);
                    vk::Result::INCOMPLETE
                }
                2 | 4 => {
                    assert!(output.is_none());
                    *count = if calls == 2 { 2 } else { 4 };
                    vk::Result::SUCCESS
                }
                5 => {
                    let output = output.unwrap();
                    assert_eq!(output.len(), 4);
                    for entry in &mut output[..3] {
                        entry.format = vk::Format::R16G16B16A16_SFLOAT;
                        entry.color_space = vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT;
                    }
                    *count = 3;
                    vk::Result::SUCCESS
                }
                _ => panic!("unexpected enumeration call"),
            }
        })
        .unwrap();
        assert_eq!(calls, 5);
        assert_eq!(formats.len(), 3);
        assert!(formats.iter().all(|entry| {
            entry.format == vk::Format::R16G16B16A16_SFLOAT
                && entry.color_space == vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT
        }));
        assert_eq!(hint.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn shrinking_formats_return_only_the_fresh_written_entries() {
        let hint = AtomicU32::new(4);
        let formats = query_surface_formats(&hint, |count, output| {
            let output = output.expect("a capacity hint skips the count query");
            assert_eq!(*count, 4);
            output[0].format = vk::Format::B8G8R8A8_SRGB;
            *count = 1;
            vk::Result::SUCCESS
        })
        .unwrap();
        assert_eq!(formats.len(), 1);
        assert_eq!(formats[0].format, vk::Format::B8G8R8A8_SRGB);
        assert_eq!(hint.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn query_errors_propagate_without_returning_partial_formats() {
        for initial_capacity in [0, 2] {
            let hint = AtomicU32::new(initial_capacity);
            for error in [
                vk::Result::ERROR_SURFACE_LOST_KHR,
                vk::Result::ERROR_OUT_OF_HOST_MEMORY,
                vk::Result::ERROR_OUT_OF_DEVICE_MEMORY,
            ] {
                let mut calls = 0;
                let result = query_surface_formats(&hint, |_, _| {
                    calls += 1;
                    error
                });
                assert_eq!(result.unwrap_err(), error);
                assert_eq!(calls, 1);
                assert_eq!(hint.load(Ordering::Relaxed), initial_capacity);
            }
        }
    }
}
