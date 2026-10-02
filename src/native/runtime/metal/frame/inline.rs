//! Only immutable constants without buffer identity can use Metal's copied bytes.
use super::*;

pub(super) const MAX_BYTES: usize = 4096;

pub(super) fn uniforms(batch: &ComputeBatch) -> Vec<bool> {
    let mut inline = vec![false; batch.resources().len()];
    for index in crate::native::runtime::compute::uniforms::immutable_uniforms(batch) {
        inline[index] = batch.resources()[index].bytes().len() <= MAX_BYTES;
    }
    inline
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(
        batch: &mut ComputeBatch,
        config: crate::native::runtime::compute::ResourceId,
        data: crate::native::runtime::compute::ResourceId,
    ) {
        let output = batch.buffer(vec![0; 16]).unwrap();
        let total = batch.buffer(vec![0; 4]).unwrap();
        // SAFETY: this test only validates/classifies bindings, never executes.
        unsafe {
            batch
                .dispatch(
                    "cumsum_prefix_chunks",
                    &[(0, config), (1, data), (2, data), (5, output), (6, total)],
                    [1, 1, 1],
                )
                .unwrap();
        }
    }

    #[test]
    fn copied_constants_preserve_storage_aliases_and_readback_identity() {
        let mut batch = ComputeBatch::new();
        let config = batch.buffer(vec![0; 16]).unwrap();
        let data = batch.buffer(vec![0; 16]).unwrap();
        prefix(&mut batch, config, data);
        prefix(&mut batch, config, data);
        assert!(uniforms(&batch)[config.index()]);
        assert!(!uniforms(&batch)[data.index()]);
        batch.readback(config).unwrap();
        assert!(!uniforms(&batch)[config.index()]);

        let mut batch = ComputeBatch::new();
        let config = batch.buffer(vec![0; 16]).unwrap();
        let data = batch.buffer(vec![0; 16]).unwrap();
        prefix(&mut batch, config, data);
        prefix(&mut batch, data, config);
        assert!(uniforms(&batch).iter().all(|inline| !inline));
    }

    #[test]
    #[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
    fn copied_constants_survive_host_drop_at_the_inline_size_boundary() -> Result<()> {
        use crate::shared::filter_config::FilterConfig;
        let mut metal = Metal::with_options(&crate::NativeContextOptions {
            validation: true,
            ..Default::default()
        })?;
        for (size, expected) in [(4096, true), (4100, false)] {
            let mut batch = ComputeBatch::new();
            let config = FilterConfig {
                width: 17,
                height: 3,
                region_width: 17,
                region_height: 3,
                pixel_count: 51,
                dispatch_width: 1,
                clear_color: u32::from_le_bytes([3, 7, 11, 173]),
                ..Default::default()
            };
            let mut bytes = bytemuck::bytes_of(&config).to_vec();
            bytes.resize(size, 0);
            let config = batch.buffer(bytes)?;
            let target = batch.texture_rgba8([17, 3], vec![0; 51 * 4])?;
            let active = batch.buffer(vec![0; 4])?;
            // SAFETY: one group covers 51 pixels, the complete region is in bounds,
            // compact selection is disabled and each pixel has one writer.
            unsafe {
                batch.dispatch(
                    "filter_clear_region",
                    &[(0, config), (3, target), (8, active)],
                    [1, 1, 1],
                )?;
            }
            batch.readback(target)?;
            for pass in batch.passes() {
                pipeline::ensure(
                    &metal.device,
                    &mut metal.libraries,
                    &mut metal.pipelines,
                    pass.shader,
                )?;
            }
            let frame = Frame::record(&metal, &batch)?;
            assert_eq!(
                matches!(frame._resources[config.index()], Resource::InlineUniform),
                expected
            );
            drop(batch);
            frame.command.commit();
            frame.wait()?;
            assert_eq!(frame.readback()?, vec![[3, 7, 11, 173].repeat(51)]);
        }
        metal.assert_valid()
    }

    #[test]
    fn copied_constants_respect_the_metal_four_kib_limit() {
        for (size, expected) in [(16, true), (4096, true), (4100, false)] {
            let mut batch = ComputeBatch::new();
            let config = batch.buffer(vec![0; size]).unwrap();
            let data = batch.buffer(vec![0; 16]).unwrap();
            prefix(&mut batch, config, data);
            assert_eq!(uniforms(&batch)[config.index()], expected);
        }
    }
}
