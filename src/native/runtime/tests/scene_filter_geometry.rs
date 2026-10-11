//! The actual scan-owned geometry rejects invalid logical draw references;
//! padding and dormant CPU uploads cannot define live layer coverage.
use super::*;
use crate::native::runtime::gpu_support;
use crate::shared::filter_config::FilterConfig;
use peniko::{Color, kurbo::Rect};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn scene_filter_mask_rejects_invalid_logical_draws_before_recording() -> Result<()> {
    let context = gpu_support::context()?;
    for count in [0, 1, 3] {
        let mut canvas = Canvas::new(32, 32, 1.0);
        for i in 0..count {
            canvas.push_rect(
                Rect::new(i as f64, 0.0, 16.0, 16.0),
                crate::Radius::ZERO,
                Color::WHITE,
            );
        }
        let mut batch = gpu_support::batch(&context);
        let scene = SceneCache::default().record(&mut batch, &canvas, None, None, 65535)?;
        let geometry = scene.filter_geometry();
        let target = batch.texture_rgba8([32, 32], vec![0; 32 * 32 * 4])?;
        let config = FilterConfig {
            width: 32,
            height: 32,
            region_width: 32,
            region_height: 32,
            ..Default::default()
        };
        let resources = batch.resources().len();
        let passes = batch.passes().len();
        for draw_ix in [count, count + 1, u32::MAX] {
            let error = geometry
                .mask(&mut batch, FilterConfig { draw_ix, ..config }, None, target)
                .expect_err("a missing layer draw must not silently become transparent coverage");
            assert!(error.to_string().contains("logical draw"));
            assert_eq!(batch.resources().len(), resources);
            assert_eq!(batch.passes().len(), passes);
        }
        if count != 0 {
            geometry.mask(
                &mut batch,
                FilterConfig {
                    draw_ix: count - 1,
                    ..config
                },
                None,
                target,
            )?;
            assert_eq!(batch.passes().len(), passes + 1);
        }
    }
    Ok(())
}
