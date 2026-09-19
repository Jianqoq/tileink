use super::{ComputeBatch, Resource, ResourceId, Texture};
use crate::native::{
    NativeTexture,
    runtime::{
        Result,
        program::filter::{self, BasicFilter},
    },
};
use crate::shared::filter_config::FilterConfig;

impl ComputeBatch {
    /// Import once per allocation so distinct cloned handles cannot hide aliasing.
    pub fn import_texture(&mut self, texture: &NativeTexture) -> Result<ResourceId> {
        if let Some(index) = self.resources.iter().position(|resource| {
            matches!(resource, Resource::Texture(existing) if existing.persistent.as_ref().is_some_and(|existing| std::rc::Rc::ptr_eq(&existing.state, &texture.state)))
        }) {
            return Ok(ResourceId { owner: self.owner, index });
        }
        let id = ResourceId {
            owner: self.owner,
            index: self.resources.len(),
        };
        self.resources.push(Resource::Texture(Texture {
            size: texture.size,
            layers: 1,
            array: false,
            bytes: Vec::new(),
            persistent: Some(texture.clone()),
        }));
        // Decide at native recording time: several CPU batches may be prepared
        // before the first submit, but only that first submit initializes pixels.
        filter::encode(
            self,
            BasicFilter::Clear,
            FilterConfig {
                width: texture.size[0],
                height: texture.size[1],
                region_width: texture.size[0],
                region_height: texture.size[1],
                ..Default::default()
            },
            None,
            None,
            id,
        )?;
        self.passes
            .last_mut()
            .expect("nonempty initialization")
            .initialization = Some(id);
        Ok(id)
    }

    pub fn skip_initialization(&self, pass: &super::Pass) -> bool {
        pass.initialization
            .is_some_and(|id| match &self.resources[id.index] {
                Resource::Texture(texture) => {
                    texture.persistent.as_ref().unwrap().state.initialized.get()
                }
                _ => unreachable!("initialization targets an image"),
            })
    }

    pub fn confirm_initialization(&self) {
        for resource in &self.resources {
            if let Resource::Texture(texture) = resource
                && let Some(texture) = &texture.persistent
            {
                texture.state.initialized.set(true);
            }
        }
    }
}
