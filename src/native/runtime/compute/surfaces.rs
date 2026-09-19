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
}

impl SurfacePool {
    pub(crate) fn new(context: &NativeContext) -> Self {
        Self {
            context: context.clone(),
            resources: SceneResourcePool::default(),
        }
    }

    fn acquire(&mut self, size: [u32; 2]) -> Result<NativeTexture> {
        let size = (size[0], size[1]);
        let texture = loop {
            match self.resources.acquire(size) {
                // A retained surface still owns its pixels. Remove its pool lease
                // instead of clearing/reusing an allocation pinned by the cache.
                Some(entry) if Rc::strong_count(&entry.allocation.state) != 1 => continue,
                Some(entry) if entry.allocation.size() == size => break entry.allocation,
                _ => break self.context.create_texture(size.0, size.1)?,
            }
        };
        self.resources.recycle(SceneResources {
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
        pool.borrow_mut().resources.begin_frame();
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
        let Some(pool) = &self.surface_pool else {
            return Ok(None);
        };
        let texture = pool.borrow_mut().acquire(size)?;
        let image = self.import_texture(&texture)?;
        // Every scratch lease starts with transparent (or requested clear) pixels,
        // even when the backing allocation contains a previous frame's output.
        filter::encode(
            self,
            BasicFilter::Clear,
            FilterConfig {
                width: size[0],
                height: size[1],
                region_width: size[0],
                region_height: size[1],
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
