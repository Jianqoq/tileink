use super::*;
use crate::native::interop::metal::{
    ContextDescriptor, EventPoint, TargetSynchronization, TextureDescriptor,
};

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn imported_target_waits_and_signals_without_blocking_submission() -> Result<()> {
    let device = MTLCreateSystemDefaultDevice().ok_or("Metal device")?;
    let queue = device.newCommandQueue().ok_or("Metal queue")?;
    let texture = memory::texture(&device, [17, 3], 1, false)?;
    let event = device.newSharedEvent().ok_or("Metal shared event")?;
    // SAFETY: this test owns the device, queue, texture and event. Host accesses
    // resume only after the declared signal and completion receipt.
    let context = unsafe {
        crate::NativeContext::from_metal(ContextDescriptor {
            device: device.clone(),
            queue,
        })?
    };
    let target = unsafe {
        context.import_metal_texture(TextureDescriptor {
            texture,
            initialized: false,
        })?
    };
    let mut renderer = crate::NativeRenderer::with_context(&context, 17, 3)?;
    renderer.set_clear_color(peniko::Color::from_rgba8(80, 40, 20, 128));
    let canvas = crate::Canvas::new(17, 3, 1.0);
    let usage = unsafe {
        crate::NativeRenderTarget::transient(&target).with_metal_synchronization(
            TargetSynchronization {
                waits: vec![EventPoint {
                    event: event.clone(),
                    value: 1,
                }],
                signals: vec![EventPoint {
                    event: event.clone(),
                    value: 2,
                }],
            },
        )
    };
    let submission = renderer.render_to_target_use(&canvas, usage)?.submission;
    // Reaching this assertion proves submission did not wait for the host event.
    let completed_before_signal = submission.is_complete()?;
    event.setSignaledValue(1);
    submission.wait()?;
    assert!(!completed_before_signal);
    assert_eq!(event.signaledValue(), 2);
    let readback = target.readback()?;
    drop(renderer);
    drop(target);
    drop(context);
    let image = readback.readback()?;
    let expected = crate::shared::image::premul_color_to_rgba8_pack(peniko::Color::from_rgba8(
        80, 40, 20, 128,
    ));
    assert_eq!(image.pixels, vec![expected; 51]);
    Ok(())
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn event_contract_rejects_duplicate_and_nonadvancing_signals() -> Result<()> {
    let device = MTLCreateSystemDefaultDevice().ok_or("Metal device")?;
    let event = device.newSharedEvent().ok_or("Metal event")?;
    let point = |value| EventPoint {
        event: event.clone(),
        value,
    };
    for value in [0, 1, 2] {
        let sync = TargetSynchronization {
            waits: vec![point(2)],
            signals: vec![point(value)],
        };
        assert!(sync.validate(&device).is_err());
    }
    assert!(
        TargetSynchronization {
            waits: vec![],
            signals: vec![point(1), point(2)]
        }
        .validate(&device)
        .is_err()
    );
    TargetSynchronization {
        waits: vec![point(2)],
        signals: vec![point(3)],
    }
    .validate(&device)?;
    Ok(())
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn incompatible_imports_and_foreign_logical_targets_are_rejected() -> Result<()> {
    let device = MTLCreateSystemDefaultDevice().ok_or("Metal device")?;
    // SAFETY: test-owned queues and resources have no outstanding host work.
    let context = unsafe {
        crate::NativeContext::from_metal(ContextDescriptor {
            device: device.clone(),
            queue: device.newCommandQueue().ok_or("Metal queue")?,
        })?
    };
    for (format, usage, levels, hazard) in [
        (
            MTLPixelFormat::BGRA8Unorm,
            MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite,
            1,
            MTLHazardTrackingMode::Tracked,
        ),
        (
            MTLPixelFormat::RGBA8Unorm,
            MTLTextureUsage::ShaderRead,
            1,
            MTLHazardTrackingMode::Tracked,
        ),
        (
            MTLPixelFormat::RGBA8Unorm,
            MTLTextureUsage::ShaderWrite,
            1,
            MTLHazardTrackingMode::Tracked,
        ),
        (
            MTLPixelFormat::RGBA8Unorm,
            MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite,
            2,
            MTLHazardTrackingMode::Tracked,
        ),
        (
            MTLPixelFormat::RGBA8Unorm,
            MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite,
            1,
            MTLHazardTrackingMode::Untracked,
        ),
    ] {
        let descriptor = MTLTextureDescriptor::new();
        descriptor.setPixelFormat(format);
        descriptor.setTextureType(MTLTextureType::Type2D);
        descriptor.setUsage(usage);
        descriptor.setStorageMode(MTLStorageMode::Private);
        descriptor.setHazardTrackingMode(hazard);
        // SAFETY: valid allocation dimensions and mip counts; only Tileink's
        // stricter import contract is intentionally violated, not Metal's API.
        unsafe {
            descriptor.setWidth(17);
            descriptor.setHeight(3);
            descriptor.setMipmapLevelCount(levels);
        }
        let texture = device
            .newTextureWithDescriptor(&descriptor)
            .ok_or("test texture")?;
        assert!(
            unsafe {
                context.import_metal_texture(TextureDescriptor {
                    texture,
                    initialized: false,
                })
            }
            .is_err()
        );
    }
    let other = crate::NativeContext::new(crate::NativeBackend::Metal, &Default::default())?;
    let foreign = other.create_texture(17, 3)?;
    let mut renderer = crate::NativeRenderer::with_context(&context, 17, 3)?;
    let canvas = crate::Canvas::new(17, 3, 1.0);
    assert!(renderer.render_to_texture(&canvas, &foreign).is_err());
    // Rejection must not poison the valid context or publish foreign history.
    let image = renderer.render_to_image(&canvas)?.readback()?;
    assert_eq!(image.pixels, vec![0; 51]);
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn failed_context_refuses_new_work_and_does_not_retire_pending_leases() -> Result<()> {
    let mut device = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    let mut batch = ComputeBatch::new();
    let buffer = batch.buffer(vec![17; 16])?;
    batch.readback(buffer)?;
    let ticket = device.submit_compute(&batch)?;
    // Inject the state reached after command failure. Real device removal is
    // nondeterministic; this checks the failure policy without resetting the GPU.
    device.failed = true;
    assert!(device.submit_compute(&batch).is_err());
    assert!(device.is_complete(&ticket).is_err());
    assert!(device.readback_batch(&ticket).is_err());
    assert!(device.assert_valid().is_err());
    assert!(device.unconfirmed());
    assert_eq!(device.pending_count(), 1);
    // Drop can prove this actual command completed and release it safely.
    Ok(())
}
