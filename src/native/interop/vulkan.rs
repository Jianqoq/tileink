use crate::native::{NativeBackend, NativeContext, NativeError};
use std::{any::Any, rc::Rc};

/// The host remains responsible for destroying its instance and logical device.
/// `owner` must keep the loader, instance, device, and queue alive until dropped.
pub struct ContextDescriptor {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub physical_device: ash::vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: ash::vk::Queue,
    pub queue_family: u32,
    /// True only if shaderSampledImageArrayNonUniformIndexing was enabled at creation.
    pub texture_tables: bool,
    /// True only when timelineSemaphore and Vulkan 1.2 or KHR_timeline_semaphore were enabled.
    pub timeline_semaphores: bool,
    /// Requires the validation layer and VK_EXT_debug_utils enabled on the instance.
    pub validation: bool,
    pub owner: Rc<dyn Any>,
}
impl NativeContext {
    /// Import a host-owned compute-capable Vulkan queue.
    ///
    /// # Safety
    /// Handles must be live, belong to this instance/device, and remain valid through
    /// `owner`. The queue must belong to `queue_family`; enabled feature/extension
    /// declarations must be accurate. The host must serialize queue access with
    /// Tileink and synchronize every imported resource before and after rendering.
    pub unsafe fn from_vulkan(descriptor: ContextDescriptor) -> Result<Self, NativeError> {
        let adapter = crate::native::runtime::adapter::Adapter::from_vulkan(descriptor)
            .map_err(NativeError::Initialization)?;
        Ok(Self::from_adapter(NativeBackend::Vulkan, adapter))
    }
}

/// A host-owned, single-mip, single-layer RGBA8 image. All four usage bits
/// (sampled, storage, transfer source, transfer destination) are required.
pub struct TextureDescriptor {
    pub image: ash::vk::Image,
    pub size: [u32; 2],
    pub initial_layout: ash::vk::ImageLayout,
    pub final_layout: ash::vk::ImageLayout,
    /// False discards undefined contents and initializes the image before use.
    pub initialized: bool,
    pub owner: Rc<dyn Any>,
}
impl NativeContext {
    /// Import an image while retaining its host allocation owner.
    ///
    /// # Safety
    /// The image must belong to this logical device, have the documented format,
    /// dimensions, usages and one sample, and remain live through `owner`. The
    /// host must synchronize access and transfer queue-family ownership to this
    /// context before ordinary rendering. Ordinary calls restore `final_layout`;
    /// an explicit TargetUse overrides incoming/outgoing scopes and queue ownership.
    /// Set `initialized` only when every pixel has defined contents. Reimport
    /// after host writes to establish a new retained-content identity.
    pub unsafe fn import_vulkan_texture(
        &self,
        descriptor: TextureDescriptor,
    ) -> Result<crate::NativeTexture, NativeError> {
        let size = descriptor.size;
        let initialized = descriptor.initialized;
        let allocation = self
            .adapter
            .import_vulkan_texture(descriptor)
            .map_err(NativeError::Initialization)?;
        Ok(crate::NativeTexture {
            state: Rc::new(crate::native::runtime::texture::State {
                allocation,
                initialized: std::cell::Cell::new(initialized),
                content_version: std::cell::Cell::new(0),
            }),
            context: self.clone(),
            size,
            layers: 1,
            array: false,
        })
    }
}

/// Native image scope at an external queue boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageState {
    pub layout: ash::vk::ImageLayout,
    pub stages: ash::vk::PipelineStageFlags,
    pub access: ash::vk::AccessFlags,
    /// QUEUE_FAMILY_IGNORED denotes this context's own queue family.
    pub queue_family: u32,
}
#[derive(Clone, Copy, Debug)]
pub enum SemaphorePoint {
    Binary(ash::vk::Semaphore),
    Timeline {
        semaphore: ash::vk::Semaphore,
        value: u64,
    },
}
impl SemaphorePoint {
    pub(crate) fn handle(self) -> ash::vk::Semaphore {
        match self {
            Self::Binary(handle) => handle,
            Self::Timeline { semaphore, .. } => semaphore,
        }
    }
    pub(crate) fn value(self) -> u64 {
        match self {
            Self::Binary(_) => 0,
            Self::Timeline { value, .. } => value,
        }
    }
}
#[derive(Clone)]
pub struct SemaphoreWait {
    pub semaphore: SemaphorePoint,
    /// Tileink widens valid wait scopes to ALL_COMMANDS to cover transfer and compute.
    pub stages: ash::vk::PipelineStageFlags,
}
#[derive(Clone)]
pub struct TargetSynchronization {
    pub incoming: ImageState,
    pub outgoing: ImageState,
    pub waits: Vec<SemaphoreWait>,
    pub signals: Vec<SemaphorePoint>,
    /// Keeps every semaphore and its device live through the submission.
    pub owner: Rc<dyn Any>,
}
impl NativeContext {
    /// Construct a consumed target use with explicit GPU dependencies.
    ///
    /// # Safety
    /// Semaphore handles/types must be accurate and live through `owner`. Binary
    /// waits consume one outstanding signal, and signals must not already be pending.
    /// Incoming scope must match actual host accesses. For a different source family,
    /// the host must first release incoming.layout -> GENERAL to this context's family;
    /// Tileink records the matching acquire. The destination must acquire the matching
    /// GENERAL -> outgoing.layout release before using an exported image.
    pub unsafe fn vulkan_target_use<'a>(
        &self,
        target: crate::NativeRenderTarget<'a>,
        synchronization: TargetSynchronization,
    ) -> Result<crate::NativeTargetUse<'a>, NativeError> {
        if self.backend() != NativeBackend::Vulkan
            || !self.adapter.same_device(&target.texture.context.adapter)
        {
            return Err(NativeError::Initialization(
                "Vulkan target use requires its registered context".into(),
            ));
        }
        if synchronization.incoming.layout == ash::vk::ImageLayout::UNDEFINED
            && target.texture.state.initialized.get()
        {
            return Err(NativeError::Initialization(
                "discarded Vulkan content requires a new uninitialized registration".into(),
            ));
        }
        Ok(crate::NativeTargetUse {
            target,
            synchronization: super::Synchronization::Vulkan(synchronization),
        })
    }
}
