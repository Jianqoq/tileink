use std::{cell::RefCell, rc::Rc};

use super::{ComputeBatch, ResourceId};
use crate::native::runtime::{
    Result,
    program::filter::{self, BasicFilter},
};
use crate::native::{NativeContext, NativeTexture};
use crate::render::scene_resources::{SceneResourcePool, SceneResources};
use crate::shared::filter_config::FilterConfig;

/// Renderer-local scratch allocation leases. The shared pool prevents siblings
/// from reusing storage before their owning command batch is resolved.
/// Retained surfaces pin their allocation: the pool skips pinned entries, so a
/// new scratch lease cannot clear pixels still serving as retained history.
pub(crate) struct SurfacePool {
    context: NativeContext,
    resources: SceneResourcePool<NativeTexture>,
    scratch_resources: SceneResourcePool<NativeTexture>,
}

impl SurfacePool {
    pub(crate) fn new(context: &NativeContext) -> Self {
        Self {
            context: context.clone(),
            resources: SceneResourcePool::default(),
            scratch_resources: SceneResourcePool::default(),
        }
    }

    fn acquire(&mut self, size: [u32; 2], scratch: bool) -> Result<NativeTexture> {
        let size = (size[0], size[1]);
        // Exact roots must not consume oversized scratch leases and discard their
        // capacity. Each allocation contract keeps its own queue-ordered pool.
        let resources = if scratch {
            &mut self.scratch_resources
        } else {
            &mut self.resources
        };
        let texture = loop {
            match resources.acquire(size) {
                // Pinned pixels cannot be overwritten. Defer the lease to the next
                // frame so cache invalidation can release it for reuse; dropping
                // the lease here permanently lost reusable allocations every frame.
                Some(entry) if Rc::strong_count(&entry.allocation.state) != 1 => {
                    resources.recycle(entry);
                }
                Some(entry) if entry.allocation.size() == size => break entry.allocation,
                Some(entry) if scratch => {
                    let current = entry.allocation.size();
                    let capacity = scratch_capacity(
                        [current.0, current.1],
                        [size.0, size.1],
                        self.context.adapter.limits().image_dimension,
                    );
                    if capacity == [current.0, current.1] {
                        break entry.allocation;
                    }
                    break self.context.create_texture(capacity[0], capacity[1])?;
                }
                _ => break self.context.create_texture(size.0, size.1)?,
            }
        };
        resources.recycle(SceneResources {
            target_size: size,
            allocation: texture.clone(),
        });
        Ok(texture)
    }
}

impl ComputeBatch {
    pub(crate) fn context(&self) -> Option<NativeContext> {
        self.surface_pool
            .as_ref()
            .map(|pool| pool.borrow().context.clone())
    }

    pub(crate) fn adapter(&self) -> Option<crate::native::runtime::adapter::Adapter> {
        self.surface_pool
            .as_ref()
            .map(|pool| pool.borrow().context.adapter.clone())
    }

    /// The preceding renderer batch must have been submitted or discarded. Queue
    /// ordering protects its GPU users; this is not a completion or CPU-wait boundary.
    pub(crate) fn with_surfaces(pool: Rc<RefCell<SurfacePool>>) -> Self {
        {
            let mut pool = pool.borrow_mut();
            pool.resources.begin_frame();
            pool.scratch_resources.begin_frame();
        }
        Self {
            surface_pool: Some(pool),
            ..Self::new()
        }
    }

    pub(crate) fn reusable_surface(
        &mut self,
        size: [u32; 2],
        clear_color: u32,
    ) -> Result<Option<ResourceId>> {
        self.reusable_surface_inner(size, clear_color, false)
    }

    pub(crate) fn reusable_scratch_surface(
        &mut self,
        size: [u32; 2],
    ) -> Result<Option<ResourceId>> {
        self.reusable_surface_inner(size, 0, true)
    }

    fn reusable_surface_inner(
        &mut self,
        size: [u32; 2],
        clear_color: u32,
        scratch: bool,
    ) -> Result<Option<ResourceId>> {
        let Some(pool) = &self.surface_pool else {
            return Ok(None);
        };
        let texture = pool.borrow_mut().acquire(size, scratch)?;
        let image = self.import_texture(&texture)?;
        let extent = texture.size();
        // Every scratch lease starts with transparent (or requested clear) pixels,
        // even when the backing allocation contains a previous frame's output.
        filter::encode(
            self,
            BasicFilter::Clear,
            FilterConfig {
                width: extent.0,
                height: extent.1,
                region_width: extent.0,
                region_height: extent.1,
                clear_color,
                ..Default::default()
            },
            None,
            None,
            image,
        )?;
        Ok(Some(image))
    }
}

// Fix resize allocation churn without changing public output extents. Internal
// scratch surfaces carry separate logical bounds; modest spare capacity amortizes
// GPU allocation/residency work, while large shrinks return unused storage.
fn scratch_capacity(current: [u32; 2], required: [u32; 2], limit: u32) -> [u32; 2] {
    if (0..2).any(|axis| current[axis] > required[axis].saturating_mul(3)) {
        return required;
    }
    std::array::from_fn(|axis| {
        if required[axis] <= current[axis] {
            current[axis]
        } else {
            required[axis].max(current[axis].saturating_add(current[axis] / 2).min(limit))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::scratch_capacity;

    #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
    #[test]
    #[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
    fn pinned_surface_rejoins_pool_after_owner_releases_it() -> super::Result<()> {
        use super::*;
        use crate::native::{NativeBackend, NativeContextOptions};
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
        for scratch in [false, true] {
            let mut pool = SurfacePool::new(&context);
            let first = pool.acquire([84, 42], scratch)?;
            let identity = Rc::downgrade(&first.state);
            pool.resources.begin_frame();
            pool.scratch_resources.begin_frame();
            let second = pool.acquire([84, 42], scratch)?;
            assert!(
                !Rc::ptr_eq(&first.state, &second.state),
                "pinned pixels must not alias"
            );
            drop(first);
            pool.resources.begin_frame();
            pool.scratch_resources.begin_frame();
            let recovered = pool.acquire([84, 42], scratch)?;
            assert!(
                std::ptr::eq(identity.as_ptr(), Rc::as_ptr(&recovered.state)),
                "a temporarily pinned allocation must return to the pool"
            );
        }
        context.check_validation()?;
        Ok(())
    }

    #[test]
    fn scratch_capacity_grows_reuses_shrinks_and_respects_device_limit() {
        assert_eq!(scratch_capacity([16, 12], [20, 14], 32), [24, 18]);
        assert_eq!(scratch_capacity([24, 18], [18, 13], 32), [24, 18]);
        assert_eq!(scratch_capacity([24, 18], [7, 13], 32), [7, 13]);
        assert_eq!(scratch_capacity([24, 18], [25, 19], 32), [32, 27]);
        // Preserve invalid requests for the context's dimension validation.
        assert_eq!(scratch_capacity([24, 18], [33, 19], 32), [33, 27]);
    }
}
