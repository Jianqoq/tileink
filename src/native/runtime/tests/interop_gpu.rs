use super::super::{adapter::Adapter, compute::ComputeBatch, texture::Allocation};
#[cfg(feature = "dx12")]
use crate::NativeBackend;
use crate::{NativeContext, NativeContextOptions};
#[cfg(feature = "vulkan")]
use std::rc::Rc;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn imported_contexts_preserve_host_images_and_restore_declared_states() -> Result {
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    for backend in [super::backend()] {
        let host = NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
                validation: true,
            },
        )?;
        let texture = host.create_texture(7, 3)?;
        // Initialize the host allocation through its own queue before handing it off.
        assert!(
            texture
                .readback()?
                .readback()?
                .pixels
                .iter()
                .all(|&pixel| pixel == 0)
        );
        let imported = Adapter::import_context_for_test(&host)?;
        let image = unsafe {
            match &texture.state.allocation {
                #[cfg(feature = "dx12")]
                Allocation::Dx12(allocation) => {
                    // Import defaults must reject impossible states before ordinary
                    // rendering can record an invalid resource barrier.
                    use windows::Win32::Graphics::Direct3D12::*;
                    for (initial_state, final_state) in [
                        (
                            D3D12_RESOURCE_STATE_RENDER_TARGET,
                            D3D12_RESOURCE_STATE_COMMON,
                        ),
                        (
                            D3D12_RESOURCE_STATE_COMMON,
                            D3D12_RESOURCE_STATE_RENDER_TARGET,
                        ),
                    ] {
                        assert!(
                            imported
                                .import_dx12_texture(
                                    crate::native_interop::dx12::TextureDescriptor {
                                        resource: allocation.resource.clone(),
                                        initialized: true,
                                        initial_state,
                                        final_state,
                                    },
                                )
                                .is_err()
                        );
                    }
                    imported
                        .import_dx12_texture(crate::native_interop::dx12::TextureDescriptor {
                        resource: allocation.resource.clone(),
                        initialized: true,
                        initial_state:
                            windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATE_COMMON,
                        final_state:
                            windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATE_COPY_SOURCE,
                    })?
                }
                #[cfg(feature = "vulkan")]
                Allocation::Vulkan(allocation) => imported.import_vulkan_texture(
                    crate::native_interop::vulkan::TextureDescriptor {
                        image: allocation.image,
                        size: [7, 3],
                        initialized: true,
                        initial_layout: ash::vk::ImageLayout::GENERAL,
                        final_layout: ash::vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        owner: Rc::new(texture.clone()),
                    },
                )?,
            }
        };
        let mut receipts = Vec::new();
        for color in [0xff231347u32, 0xff1d710b] {
            let mut batch = ComputeBatch::new();
            let source = batch.texture_rgba8([7, 3], color.to_le_bytes().repeat(21))?;
            let destination = batch.import_texture(&image)?;
            batch.copy_texture(super::super::compute::TextureCopy {
                source,
                destination,
                source_origin: [0; 3],
                destination_origin: [0; 3],
                extent: [7, 3, 1],
            })?;
            batch.readback(destination)?;
            receipts.push((
                color,
                imported
                    .adapter
                    .submit_compute(&batch)
                    .map_err(|e| format!("{e:?}"))?,
            ));
        }
        // Submitted work owns the imported allocation even after all API handles drop.
        drop(image);
        drop(texture);
        drop(imported);
        for (color, receipt) in receipts.into_iter().rev() {
            assert_eq!(receipt.readback()?, vec![color.to_le_bytes().repeat(21)]);
        }
        host.check_validation()?;
        // Destruction of the imported context must not destroy the host device.
        assert_eq!(
            host.create_texture(2, 2)?.readback()?.readback()?.pixels,
            vec![0; 4]
        );
        host.check_validation()?;
    }
    Ok(())
}

#[cfg(feature = "dx12")]
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn dx12_target_use_orders_external_queues_even_without_damage() -> Result {
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    use crate::native_interop::dx12::{FencePoint, TargetSynchronization};
    use crate::{
        NativeRenderTarget, NativeRenderer, NativeTargetState, RetainedNodeId, RetainedScene,
    };
    use windows::Win32::{Foundation::HANDLE, Graphics::Direct3D12::*};
    let context = NativeContext::new(
        NativeBackend::Dx12,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let device = context.adapter.dx12_device_for_test();
    let queue: ID3D12CommandQueue = unsafe {
        device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        })?
    };
    let before: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE)? };
    let after: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE)? };
    let consumed: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE)? };
    let texture = context.create_texture(7, 3)?;
    let scene = RetainedScene::new(7, 3, 1.0, RetainedNodeId::for_owner(870_001))?;
    let mut renderer = NativeRenderer::with_context(&context, 7, 3)?;
    let rejected = unsafe {
        context.dx12_target_use(
            NativeRenderTarget::from(&texture),
            TargetSynchronization {
                incoming: D3D12_RESOURCE_STATE_COMMON,
                outgoing: D3D12_RESOURCE_STATE_RENDER_TARGET,
                waits: vec![],
                signals: vec![FencePoint {
                    fence: after.clone(),
                    value: 1,
                }],
            },
        )?
    };
    assert!(
        renderer
            .render_retained_to_target_use(&scene, rejected)
            .is_err()
    );
    assert!(!texture.state.initialized.get());
    assert_eq!(unsafe { after.GetCompletedValue() }, 0);
    let mut incoming = D3D12_RESOURCE_STATE_COMMON;
    for value in 1..=2 {
        let outgoing = if value == 1 {
            D3D12_RESOURCE_STATE_COPY_SOURCE
        } else {
            D3D12_RESOURCE_STATE_COMMON
        };
        unsafe {
            queue.Signal(&before, value)?;
        }
        let usage = unsafe {
            context.dx12_target_use(
                NativeRenderTarget::from(&texture),
                TargetSynchronization {
                    incoming,
                    outgoing,
                    waits: vec![FencePoint {
                        fence: before.clone(),
                        value,
                    }],
                    signals: vec![FencePoint {
                        fence: after.clone(),
                        value,
                    }],
                },
            )?
        };
        let frame = renderer.render_retained_to_target_use(&scene, usage)?;
        assert_eq!(frame.outgoing, NativeTargetState { state: outgoing });
        unsafe {
            queue.Wait(&after, value)?;
            queue.Signal(&consumed, value)?;
            consumed.SetEventOnCompletion(value, HANDLE::default())?;
        }
        frame.submission.wait()?;
        if value == 2 {
            assert_eq!(renderer.incremental_render_stats().dirty_tiles, 0);
        }
        incoming = outgoing;
    }
    assert_eq!(texture.readback()?.readback()?.pixels, vec![0; 21]);
    context.check_validation()?;
    Ok(())
}

#[cfg(feature = "dx12")]
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn dx12_target_use_signal_failure_does_not_publish_state_or_history() -> Result {
    if super::super::isolation::run(
        "native::runtime::lifecycle_gpu_tests::interop::dx12_target_use_signal_failure_does_not_publish_state_or_history",
    )? {
        return Ok(());
    }
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    use crate::native_interop::dx12::{FencePoint, TargetSynchronization};
    use windows::Win32::Graphics::Direct3D12::*;
    let context = NativeContext::new(
        NativeBackend::Dx12,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let done: ID3D12Fence = unsafe {
        context
            .adapter
            .dx12_device_for_test()
            .CreateFence(0, D3D12_FENCE_FLAG_NONE)?
    };
    let target = context.create_texture(7, 3)?;
    let canvas = crate::Canvas::new(7, 3, 1.0);
    let mut renderer = crate::NativeRenderer::with_context(&context, 7, 3)?;
    let usage = unsafe {
        context.dx12_target_use(
            (&target).into(),
            TargetSynchronization {
                incoming: D3D12_RESOURCE_STATE_COMMON,
                outgoing: D3D12_RESOURCE_STATE_COPY_SOURCE,
                waits: vec![],
                signals: vec![FencePoint {
                    fence: done.clone(),
                    value: 1,
                }],
            },
        )?
    };
    context.adapter.inject_dx12_signal_failure_for_test();
    assert!(matches!(
        renderer.render_to_target_use(&canvas, usage),
        Err(crate::NativeError::SubmissionUnconfirmed(_))
    ));
    assert!(!target.state.initialized.get());
    assert_eq!(target.state.content_version.get(), 0);
    assert_eq!(unsafe { done.GetCompletedValue() }, 0);
    assert!(
        renderer.render(&canvas).is_err(),
        "an older receipt cannot prove an unfenced use safe"
    );
    Ok(())
}
