//! Presentation is a channel-format conversion, not compositing or color grading.
use super::*;
#[test]
#[ignore = "requires physical Metal GPU; run with MTL_DEBUG_LAYER=1"]
fn rgba_to_bgra_preserves_every_channel_and_edge_pixel() -> Result {
    let device = MTLCreateSystemDefaultDevice().ok_or("Metal device")?;
    let queue = device.newCommandQueue().ok_or("Metal queue")?;
    let pipeline = present::pipeline(&device)?;
    for width in [1usize, 17, 257] {
        let height = 3;
        let pixels: Vec<u8> = (0..width * height)
            .flat_map(|i| {
                [
                    (i * 17) as u8,
                    (i * 31 + 5) as u8,
                    (i * 73 + 19) as u8,
                    i as u8,
                ]
            })
            .collect();
        let desc = MTLTextureDescriptor::new();
        desc.setTextureType(MTLTextureType::Type2D);
        // SAFETY: all test dimensions are positive and below Metal family limits.
        unsafe {
            desc.setWidth(width);
            desc.setHeight(height);
        }
        desc.setPixelFormat(MTLPixelFormat::RGBA8Unorm);
        desc.setStorageMode(MTLStorageMode::Shared);
        desc.setUsage(MTLTextureUsage::ShaderRead);
        let source = device.newTextureWithDescriptor(&desc).ok_or("source")?;
        let extent = MTLSize {
            width,
            height,
            depth: 1,
        };
        let origin = MTLOrigin { x: 0, y: 0, z: 0 };
        // SAFETY: the CPU owns initialized, tightly packed bytes for every texel;
        // source has shared storage and is not yet referenced by submitted work.
        unsafe {
            source.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                MTLRegion {
                    origin,
                    size: extent,
                },
                0,
                std::ptr::NonNull::new(pixels.as_ptr().cast_mut().cast()).unwrap(),
                width * 4,
            );
        }
        desc.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        desc.setStorageMode(MTLStorageMode::Private);
        desc.setUsage(MTLTextureUsage::RenderTarget);
        let destination = device
            .newTextureWithDescriptor(&desc)
            .ok_or("destination")?;
        let pitch = (width * 4).next_multiple_of(256);
        let padding = vec![0xa5u8; pitch * height];
        // SAFETY: Metal copies the complete initialized byte slice immediately.
        let output = unsafe {
            device.newBufferWithBytes_length_options(
                std::ptr::NonNull::new(padding.as_ptr().cast_mut().cast()).unwrap(),
                padding.len(),
                MTLResourceOptions::StorageModeShared,
            )
        }
        .ok_or("readback")?;
        let command = queue.commandBuffer().ok_or("command")?;
        present::encode(&command, &pipeline, &source, &destination)?;
        let blit = command.blitCommandEncoder().ok_or("blit")?;
        // SAFETY: complete texture region, aligned rows and sufficient buffer
        // capacity. Tracked encoder boundaries order render writes before copy.
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(&destination,0,0,origin,extent,&output,0,pitch,pitch*height);
        }
        blit.endEncoding();
        command.commit();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if command.status() == MTLCommandBufferStatus::Completed {
                break;
            }
            if command.status() == MTLCommandBufferStatus::Error {
                return Err(format!("presentation test: {:?}", command.error()).into());
            }
            if std::time::Instant::now() >= deadline {
                return Err("presentation test timeout".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // SAFETY: completed copy initialized every pixel; only defined row bytes
        // are read, and the mapped allocation remains alive throughout this view.
        let bytes = unsafe {
            std::slice::from_raw_parts(output.contents().as_ptr().cast::<u8>(), pitch * height)
        };
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) * 4;
                let j = y * pitch + x * 4;
                assert_eq!(
                    &bytes[j..j + 4],
                    &[pixels[i + 2], pixels[i + 1], pixels[i], pixels[i + 3]],
                    "pixel {x},{y} of {width}x{height}"
                );
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires physical Metal GPU; run with MTL_DEBUG_LAYER=1"]
fn completed_host_command_does_not_retire_incomplete_rendering() -> Result {
    let device = MTLCreateSystemDefaultDevice().ok_or("device")?;
    let queue = device.newCommandQueue().ok_or("queue")?;
    let event = device.newSharedEvent().ok_or("event")?;
    let context = unsafe {
        NativeContext::from_metal(ContextDescriptor {
            device: device.clone(),
            queue: device.newCommandQueue().ok_or("renderer queue")?,
        })?
    };
    let texture = context.create_texture(1, 1)?;
    let mut renderer = tileink::NativeRenderer::with_context(&context, 1, 1)?;
    // SAFETY: this test alone controls the event and signals it before teardown.
    let usage = unsafe {
        NativeRenderTarget::transient(&texture).with_metal_synchronization(TargetSynchronization {
            waits: vec![EventPoint {
                event: event.clone(),
                value: 1,
            }],
            signals: vec![],
        })
    };
    let rendering = renderer
        .render_to_target_use(&tileink::Canvas::new(1, 1, 1.0), usage)?
        .submission;
    let command = queue.commandBuffer().ok_or("host command")?;
    command.commit();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while command.status() != MTLCommandBufferStatus::Completed {
        if command.status() == MTLCommandBufferStatus::Error
            || std::time::Instant::now() >= deadline
        {
            event.setSignaledValue(1);
            return Err("host command did not complete".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let frame = Frame { command, rendering };
    let complete = frame.complete();
    event.setSignaledValue(1);
    assert!(
        !complete?,
        "a host completion must not imply rendering completion"
    );
    frame.rendering.wait()?;
    Ok(())
}
