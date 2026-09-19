use super::super::{adapter::Adapter, texture::Allocation};
use crate::native_interop::vulkan::{
    ContextDescriptor, ImageState, SemaphorePoint, SemaphoreWait, TargetSynchronization,
};
use crate::{
    NativeBackend, NativeContext, NativeContextOptions, NativeRenderTarget, NativeRenderer,
    RetainedNodeId, RetainedScene,
};
use ash::vk;
use std::rc::Rc;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Host {
    // The baseline keeps the loader/instance alive; this separate logical device
    // enables timeline semaphores and queues in both tested families.
    baseline: ContextDescriptor,
    device: ash::Device,
    queue: vk::Queue,
    family: u32,
    external: vk::Queue,
    external_family: u32,
    pool: vk::CommandPool,
    ready: vk::Semaphore,
    done: vk::Semaphore,
    fence: vk::Fence,
}
impl Host {
    fn new(timeline: bool) -> Result<Rc<Self>> {
        let base = NativeContext::new(
            NativeBackend::Vulkan,
            &NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
                validation: true,
            },
        )?;
        let baseline = Adapter::vulkan_descriptor_for_test(&base);
        unsafe {
            let families = baseline
                .instance
                .get_physical_device_queue_family_properties(baseline.physical_device);
            let family = baseline.queue_family;
            let external_family = families
                .iter()
                .enumerate()
                .find(|(index, props)| {
                    *index as u32 != family
                        && props.queue_count > 0
                        && props
                            .queue_flags
                            .intersects(vk::QueueFlags::COMPUTE | vk::QueueFlags::GRAPHICS)
                })
                .map(|(index, _)| index as u32)
                .ok_or("ownership clear test requires a second compute-capable queue family")?;
            let priorities = [1.0];
            let queues = [family, external_family].map(|family| {
                vk::DeviceQueueCreateInfo::default()
                    .queue_family_index(family)
                    .queue_priorities(&priorities)
            });
            let extensions = [
                ash::ext::descriptor_indexing::NAME.as_ptr(),
                ash::khr::timeline_semaphore::NAME.as_ptr(),
            ];
            let mut indexing = vk::PhysicalDeviceDescriptorIndexingFeatures::default()
                .shader_sampled_image_array_non_uniform_indexing(true);
            let mut timelines =
                vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(timeline);
            let device = baseline.instance.create_device(
                baseline.physical_device,
                &vk::DeviceCreateInfo::default()
                    .queue_create_infos(&queues)
                    .enabled_extension_names(&extensions)
                    .push_next(&mut indexing)
                    .push_next(&mut timelines),
                None,
            )?;
            let mut this = Self {
                queue: device.get_device_queue(family, 0),
                external: device.get_device_queue(external_family, 0),
                device,
                baseline,
                family,
                external_family,
                pool: vk::CommandPool::null(),
                ready: vk::Semaphore::null(),
                done: vk::Semaphore::null(),
                fence: vk::Fence::null(),
            };
            this.pool = this.device.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(external_family),
                None,
            )?;
            for target in [&mut this.ready, &mut this.done] {
                let mut kind = vk::SemaphoreTypeCreateInfo::default().semaphore_type(if timeline {
                    vk::SemaphoreType::TIMELINE
                } else {
                    vk::SemaphoreType::BINARY
                });
                *target = this.device.create_semaphore(
                    &vk::SemaphoreCreateInfo::default().push_next(&mut kind),
                    None,
                )?;
            }
            this.fence = this
                .device
                .create_fence(&vk::FenceCreateInfo::default(), None)?;
            Ok(Rc::new(this))
        }
    }
    fn context(self: &Rc<Self>, timeline: bool) -> Result<NativeContext> {
        Ok(unsafe {
            NativeContext::from_vulkan(ContextDescriptor {
                entry: self.baseline.entry.clone(),
                instance: self.baseline.instance.clone(),
                physical_device: self.baseline.physical_device,
                device: self.device.clone(),
                queue: self.queue,
                queue_family: self.family,
                texture_tables: true,
                timeline_semaphores: timeline,
                validation: true,
                owner: self.clone(),
            })?
        })
    }
    fn barrier(
        &self,
        command: vk::CommandBuffer,
        image: vk::Image,
        old: vk::ImageLayout,
        new: vk::ImageLayout,
        families: [u32; 2],
        access: [vk::AccessFlags; 2],
    ) {
        unsafe {
            self.device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(image)
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .level_count(1)
                            .layer_count(1),
                    )
                    .old_layout(old)
                    .new_layout(new)
                    .src_queue_family_index(families[0])
                    .dst_queue_family_index(families[1])
                    .src_access_mask(access[0])
                    .dst_access_mask(access[1])],
            );
        }
    }
    fn commands(&self, image: vk::Image, first: bool) -> Result<vk::CommandBuffer> {
        unsafe {
            let command = self.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
            self.device
                .begin_command_buffer(command, &vk::CommandBufferBeginInfo::default())?;
            if first {
                self.barrier(
                    command,
                    image,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    [vk::QUEUE_FAMILY_IGNORED; 2],
                    [vk::AccessFlags::empty(), vk::AccessFlags::TRANSFER_WRITE],
                );
                self.device.cmd_clear_color_image(
                    command,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &vk::ClearColorValue {
                        float32: [1.0, 0.0, 0.0, 1.0],
                    },
                    &[vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .level_count(1)
                        .layer_count(1)],
                );
            } else {
                self.barrier(
                    command,
                    image,
                    vk::ImageLayout::GENERAL,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    [self.family, self.external_family],
                    [vk::AccessFlags::empty(), vk::AccessFlags::TRANSFER_READ],
                );
            }
            self.barrier(
                command,
                image,
                if first {
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL
                } else {
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL
                },
                vk::ImageLayout::GENERAL,
                [self.external_family, self.family],
                [
                    if first {
                        vk::AccessFlags::TRANSFER_WRITE
                    } else {
                        vk::AccessFlags::TRANSFER_READ
                    },
                    vk::AccessFlags::empty(),
                ],
            );
            self.device.end_command_buffer(command)?;
            Ok(command)
        }
    }
    fn submit(
        &self,
        command: Option<vk::CommandBuffer>,
        wait: u64,
        signal: u64,
        timeline: bool,
        last: bool,
    ) -> Result {
        let commands: Vec<_> = command.into_iter().collect();
        let waits = if wait == 0 { vec![] } else { vec![self.done] };
        let stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; waits.len()];
        let signals = if signal == 0 {
            vec![]
        } else {
            vec![self.ready]
        };
        let wait_values = vec![wait; waits.len()];
        let signal_values = vec![signal; signals.len()];
        let mut values = vk::TimelineSemaphoreSubmitInfo::default()
            .wait_semaphore_values(&wait_values)
            .signal_semaphore_values(&signal_values);
        let mut submit = vk::SubmitInfo::default()
            .command_buffers(&commands)
            .wait_semaphores(&waits)
            .wait_dst_stage_mask(&stages)
            .signal_semaphores(&signals);
        if timeline {
            submit = submit.push_next(&mut values);
        }
        unsafe {
            self.device.queue_submit(
                self.external,
                &[submit],
                if last { self.fence } else { vk::Fence::null() },
            )?;
        }
        Ok(())
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_semaphore(self.ready, None);
            self.device.destroy_semaphore(self.done, None);
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_device(None);
        }
    }
}
#[test]
#[ignore = "requires pinned GPU with two queue families and timeline support"]
fn vulkan_target_use_transfers_ownership_and_consumes_binary_or_timeline_waits() -> Result {
    for timeline in [false, true] {
        let host = Host::new(timeline)?;
        let context = host.context(timeline)?;
        let texture = context.create_texture(7, 3)?;
        let Allocation::Vulkan(image) = &texture.state.allocation else {
            unreachable!()
        };
        let raw = image.image;
        let scene = RetainedScene::new(7, 3, 1.0, RetainedNodeId::for_owner(870_002))?;
        let mut renderer = NativeRenderer::with_context(&context, 7, 3)?;
        let point = |semaphore, value| {
            if timeline {
                SemaphorePoint::Timeline { semaphore, value }
            } else {
                SemaphorePoint::Binary(semaphore)
            }
        };
        let mut receipts = Vec::new();
        for value in 1..=2 {
            let command = host.commands(raw, value == 1)?;
            host.submit(Some(command), value - 1, value, timeline, false)?;
            let incoming = ImageState {
                layout: if value == 1 {
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL
                } else {
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL
                },
                stages: vk::PipelineStageFlags::TRANSFER,
                access: if value == 1 {
                    vk::AccessFlags::TRANSFER_WRITE
                } else {
                    vk::AccessFlags::TRANSFER_READ
                },
                queue_family: host.external_family,
            };
            let outgoing = if value == 1 {
                ImageState {
                    layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    stages: vk::PipelineStageFlags::TRANSFER,
                    access: vk::AccessFlags::TRANSFER_READ,
                    queue_family: host.external_family,
                }
            } else {
                ImageState {
                    layout: vk::ImageLayout::GENERAL,
                    stages: vk::PipelineStageFlags::ALL_COMMANDS,
                    access: vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
                    queue_family: vk::QUEUE_FAMILY_IGNORED,
                }
            };
            let synchronization = TargetSynchronization {
                incoming,
                outgoing,
                waits: vec![SemaphoreWait {
                    semaphore: point(host.ready, value),
                    stages: vk::PipelineStageFlags::ALL_COMMANDS,
                }],
                signals: vec![point(host.done, value)],
                owner: host.clone(),
            };
            if value == 1 {
                let mut invalid = synchronization.clone();
                invalid.outgoing.access = vk::AccessFlags::SHADER_WRITE;
                let rejected = unsafe {
                    context.vulkan_target_use(NativeRenderTarget::from(&texture), invalid)?
                };
                assert!(
                    renderer
                        .render_retained_to_target_use(&scene, rejected)
                        .is_err()
                );
                assert!(
                    !texture.state.initialized.get(),
                    "rejection must not publish contents or consume the ready semaphore"
                );
            }
            let usage = unsafe {
                context.vulkan_target_use(NativeRenderTarget::from(&texture), synchronization)?
            };
            let frame = match renderer.render_retained_to_target_use(&scene, usage) {
                Ok(frame) => frame,
                Err(error) => {
                    eprintln!(
                        "target use failed: timeline={timeline}, frame={value}: {error}; validation: {:?}",
                        context.check_validation()
                    );
                    eprintln!(
                        "host validation: {:?}",
                        host.baseline
                            .owner
                            .downcast_ref::<NativeContext>()
                            .unwrap()
                            .check_validation()
                    );
                    return Err(error.into());
                }
            };
            assert_eq!(frame.outgoing, crate::NativeTargetState::Vulkan(outgoing));
            if value == 2 {
                assert_eq!(renderer.incremental_render_stats().dirty_tiles, 0);
            }
            receipts.push(frame.submission);
        }
        host.submit(None, 2, 0, timeline, true)?;
        unsafe {
            host.device
                .wait_for_fences(&[host.fence], true, 30_000_000_000)?;
        }
        for receipt in receipts.into_iter().rev() {
            receipt.wait()?;
        }
        assert_eq!(texture.readback()?.readback()?.pixels, vec![0; 21]);
        context.check_validation()?;
        host.baseline
            .owner
            .downcast_ref::<NativeContext>()
            .unwrap()
            .check_validation()?;
    }
    Ok(())
}
