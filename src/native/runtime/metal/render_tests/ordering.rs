//! Compute runs must end before rendering, copying, and readback. Exercise the
//! transitions with actual attachment fetch, not merely encoder-kind assertions.
use super::*;
use crate::native::runtime::{
    compute::TextureCopy,
    program::filter::{self, BasicFilter},
};
use crate::shared::filter_config::FilterConfig;

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn compute_render_copy_transitions_preserve_loaded_pixels() -> Result<()> {
    let mut metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for sparse in [false, true] {
        let size = [65, 49];
        let viewport = [33, 35];
        let active = [8, 0, 4];
        let (mut batch, _) = solid_tiles(
            size,
            viewport,
            sparse.then_some(active.as_slice()),
            true,
            TileOptions::default(),
        )?;
        let target = batch.outputs()[0];
        let fine = &batch.passes()[0];
        let bindings: Vec<_> = fine.bindings.iter().map(|(b, id)| (b.slot, *id)).collect();
        let grid = fine.grid;
        let bytes = (size[0] * size[1] * 4) as usize;
        let temporary = batch.texture_rgba8(size, vec![0; bytes])?;
        let snapshot = batch.texture_rgba8(size, vec![0; bytes])?;
        let config = FilterConfig {
            width: size[0],
            height: size[1],
            region_width: size[0],
            region_height: size[1],
            ..Default::default()
        };
        // First fine draw leaves opaque pixels; compute reads those writes and
        // turns the whole backing texture black before the second fine draw.
        filter::encode(
            &mut batch,
            BasicFilter::SourceAlpha,
            config,
            None,
            Some(target),
            temporary,
        )?;
        filter::encode(
            &mut batch,
            BasicFilter::Copy,
            config,
            None,
            Some(temporary),
            target,
        )?;
        // SAFETY: the original validated fine bindings/grid remain alive and
        // unchanged. Its unique tile list and particle/spill ranges are reused.
        unsafe {
            batch.dispatch("fine_tile_main", &bindings, grid)?;
        }
        batch.copy_texture(TextureCopy {
            source: target,
            destination: snapshot,
            source_origin: [0; 3],
            destination_origin: [0; 3],
            extent: [size[0], size[1], 1],
        })?;
        filter::encode(
            &mut batch,
            BasicFilter::Clear,
            FilterConfig {
                clear_color: u32::from_le_bytes([7, 11, 19, 255]),
                ..config
            },
            None,
            None,
            target,
        )?;
        filter::encode(
            &mut batch,
            BasicFilter::SourceAlpha,
            config,
            None,
            Some(snapshot),
            temporary,
        )?;
        batch.readback(snapshot)?;
        batch.readback(temporary)?;
        let mut expected = [0, 0, 0, 255].repeat(bytes / 4);
        for y in 0..viewport[1] {
            for x in 0..viewport[0] {
                if !sparse || active.contains(&(y / 16 * viewport[0].div_ceil(16) + x / 16)) {
                    expected[((y * size[0] + x) * 4) as usize] = 128;
                }
            }
        }
        let ticket = metal.submit_compute(&batch)?;
        assert_eq!(
            metal.readback_batch(&ticket)?,
            vec![
                [7, 11, 19, 255].repeat(bytes / 4),
                expected,
                [0, 0, 0, 255].repeat(bytes / 4),
            ]
        );
    }
    metal.assert_valid()
}
