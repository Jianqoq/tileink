use super::super::{
    Result,
    compute::ComputeBatch,
    program::filter::{self, BasicFilter},
};
use crate::shared::filter_config::FilterConfig;
use crate::{NativeBackend, NativeContext, NativeContextOptions};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_persistent_texture_preserves_untouched_pixels_across_submissions() -> Result<()> {
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        let options = NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        };
        let context = NativeContext::new(backend, &options)?;
        let texture = context.create_texture(7, 3)?;
        let mut first = ComputeBatch::new();
        let image = first.import_texture(&texture)?;
        assert_eq!(image, first.import_texture(&texture.clone())?);
        filter::encode(
            &mut first,
            BasicFilter::Clear,
            FilterConfig {
                width: 7,
                height: 3,
                region_width: 3,
                region_height: 2,
                clear_color: u32::from_le_bytes([71, 19, 23, 255]),
                ..Default::default()
            },
            None,
            None,
            image,
        )?;
        let mut second = ComputeBatch::new();
        let image = second.import_texture(&texture)?;
        filter::encode(
            &mut second,
            BasicFilter::Clear,
            FilterConfig {
                width: 7,
                height: 3,
                region_x0: 2,
                region_y0: 1,
                region_width: 2,
                region_height: 1,
                clear_color: u32::from_le_bytes([11, 113, 29, 255]),
                ..Default::default()
            },
            None,
            None,
            image,
        )?;
        second.readback(image)?;
        let other = NativeContext::new(backend, &options)?;
        assert!(other.adapter.submit_compute(&second).is_err());
        let first_receipt = context
            .adapter
            .submit_compute(&first)
            .map_err(|e| format!("{e:?}"))?;
        let second_receipt = context
            .adapter
            .submit_compute(&second)
            .map_err(|e| format!("{e:?}"))?;
        let mut third = ComputeBatch::new();
        let third_image = third.import_texture(&texture)?;
        third.readback(third_image)?;
        let third_receipt = context
            .adapter
            .submit_compute(&third)
            .map_err(|e| format!("{e:?}"))?;
        drop(third);
        drop(first);
        drop(second);
        drop(texture);
        let third_output = third_receipt.readback()?;
        let output = second_receipt.readback()?;
        assert_eq!(third_output, output);
        for y in 0..3 {
            for x in 0..7 {
                let expected = if y == 1 && (2..4).contains(&x) {
                    [11, 113, 29, 255]
                } else if x < 3 && y < 2 {
                    [71, 19, 23, 255]
                } else {
                    [0, 0, 0, 0]
                };
                assert_eq!(&output[0][(y * 7 + x) * 4..(y * 7 + x + 1) * 4], expected);
            }
        }
        first_receipt.readback()?;
        // Exercise COPY_SOURCE/COPY_DEST followed by UAV writes in later submissions.
        // Both allocations must return to the shared queue's persistent state.
        let source = context.create_texture(7, 3)?;
        let destination = context.create_texture(7, 3)?;
        let mut copy = ComputeBatch::new();
        let uploaded = copy.texture_rgba8([7, 3], output[0].clone())?;
        let source_id = copy.import_texture(&source)?;
        let destination_id = copy.import_texture(&destination)?;
        for (source, destination) in [(uploaded, source_id), (source_id, destination_id)] {
            copy.copy_texture(super::super::compute::TextureCopy {
                source,
                destination,
                source_origin: [0; 3],
                destination_origin: [0; 3],
                extent: [7, 3, 1],
            })?;
        }
        let copied = context.submit_compute(&copy)?;
        let mut edit = ComputeBatch::new();
        for texture in [&source, &destination] {
            let image = edit.import_texture(texture)?;
            filter::encode(
                &mut edit,
                BasicFilter::Clear,
                FilterConfig {
                    width: 7,
                    height: 3,
                    region_x0: 6,
                    region_y0: 2,
                    region_width: 1,
                    region_height: 1,
                    clear_color: u32::from_le_bytes([13, 17, 23, 255]),
                    ..Default::default()
                },
                None,
                None,
                image,
            )?;
        }
        let edited = context.submit_compute(&edit)?;
        let mut expected = output[0].clone();
        expected[80..84].copy_from_slice(&[13, 17, 23, 255]);
        for texture in [&source, &destination] {
            let image = texture.readback()?.readback()?;
            assert_eq!(bytemuck::cast_slice::<_, u8>(&image.pixels), expected);
        }
        edited.wait()?;
        copied.wait()?;
        context.check_validation()?;
    }
    Ok(())
}
