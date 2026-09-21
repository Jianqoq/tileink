use super::*;

#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn filter_scene_buffers_survive_between_frames_without_sibling_aliasing() -> Result<()> {
    use crate::native::runtime::compute::{Resource, SurfacePool};
    use crate::native::{NativeBackend, NativeContext, NativeContextOptions};
    use std::{cell::RefCell, rc::Rc};
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU").expect("pin the GPU")),
            validation: false,
        },
    )?;
    let pool = Rc::new(RefCell::new(SurfacePool::new(&context)));
    let mut canvas = Canvas::new(64, 32, 1.0);
    for (x, color) in [
        (2.0, peniko::Color::from_rgb8(210, 20, 50)),
        (34.0, peniko::Color::from_rgb8(20, 190, 80)),
    ] {
        let rect = peniko::kurbo::Rect::new(x, 4.0, x + 20.0, 24.0);
        canvas.push_filter_layer(
            crate::Filter::Blur {
                std_dev_x: 2.0,
                std_dev_y: 2.0,
                sampling: Default::default(),
            },
            crate::Region::rect(rect, crate::Radius::ZERO),
        );
        canvas.push_rect(rect, crate::Radius::ZERO, color);
        canvas.pop_layer();
    }
    let mut recording = Recording::default();
    let mut previous = Vec::new();
    let mut pixels = None;
    for frame in 0..3 {
        let mut batch = ComputeBatch::with_surfaces(pool.clone());
        let target = recording.record(
            &mut batch,
            &canvas,
            &ImageResourceStore::default(),
            None,
            context.adapter.limits(),
            Default::default(),
        )?;
        let buffers: Vec<_> = batch
            .resources()
            .iter()
            .filter_map(|resource| match resource {
                Resource::PersistentBuffer(upload) => Some(upload.buffer.state.clone()),
                _ => None,
            })
            .collect();
        assert!(!buffers.is_empty());
        if frame > 0 {
            assert_eq!(buffers.len(), previous.len());
            assert!(
                buffers.iter().zip(&previous).all(|(a, b)| Rc::ptr_eq(a, b)),
                "unchanged filter scenes must reuse their persistent buffers"
            );
        }
        previous = buffers;
        batch.readback(target)?;
        let rendered = context.adapter.submit_compute(&batch).unwrap().readback()?;
        if let Some(expected) = &pixels {
            assert_eq!(&rendered, expected);
        }
        pixels = Some(rendered);
    }
    Ok(())
}
