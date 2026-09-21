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
    pub(crate) fn persistent_texture(&self, id: ResourceId) -> Result<Option<&NativeTexture>> {
        self.size(id)?;
        let Resource::Texture(texture) = &self.resources[id.index] else {
            return Err("native image lease requires a texture".into());
        };
        Ok(texture.persistent.as_ref())
    }
    /// Import once per allocation so distinct cloned handles cannot hide aliasing.
    pub fn import_texture(&mut self, texture: &NativeTexture) -> Result<ResourceId> {
        self.import_texture_inner(texture, true)
    }
    pub(super) fn import_texture_inner(
        &mut self,
        texture: &NativeTexture,
        initialize: bool,
    ) -> Result<ResourceId> {
        if let Some(index) = self.resources.iter().position(|resource| {
            matches!(resource, Resource::Texture(existing) if existing.persistent.as_ref().is_some_and(|existing| std::rc::Rc::ptr_eq(&existing.state, &texture.state)))
        }) {
            return Ok(ResourceId { owner: self.owner, index });
        }
        if initialize && texture.array && !texture.state.initialized.get() {
            return Err("new array textures require a complete GPU copy before sampling".into());
        }
        let id = ResourceId {
            owner: self.owner,
            index: self.resources.len(),
        };
        self.resources.push(Resource::Texture(Texture {
            size: texture.size,
            layers: texture.layers,
            array: texture.array,
            bytes: Vec::new().into(),
            persistent: Some(texture.clone()),
        }));
        // Already initialized storage needs no clear command or uniform upload.
        // Uninitialized imports still defer the final decision to native recording:
        // batches prepared together may observe an earlier accepted initialization.
        if !initialize || texture.state.initialized.get() {
            return Ok(id);
        }
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

    pub fn confirm_submission(&self) {
        self.accepted.set(true);
        for resource in &self.resources {
            if let Resource::PersistentBuffer(upload) = resource {
                upload.buffer.state.initialized.set(true);
                upload.accepted.set(true);
            }
        }
        let mut written: Vec<_> = self.resources.iter().map(|resource| {
            matches!(resource, Resource::Texture(texture) if !texture.bytes.is_empty())
        }).collect();
        for command in &self.commands {
            match command {
                super::Command::CopyTexture(copy) => written[copy.destination.index()] = true,
                super::Command::Dispatch(index) => {
                    let pass = &self.passes[*index];
                    if !self.skip_initialization(pass) {
                        for (binding, resource) in &pass.bindings {
                            if binding.kind.writable() {
                                written[resource.index()] = true;
                            }
                        }
                    }
                }
            }
        }
        for (index, resource) in self.resources.iter().enumerate() {
            if let Resource::Texture(texture) = resource
                && let Some(texture) = &texture.persistent
            {
                texture.state.initialized.set(true);
                match &texture.state.allocation {
                    #[cfg(feature = "metal")]
                    crate::native::runtime::texture::Allocation::Metal(_) => {}
                    #[cfg(feature = "dx12")]
                    crate::native::runtime::texture::Allocation::Dx12(allocation) => {
                        allocation.state.set(allocation.final_state)
                    }
                    #[cfg(feature = "vulkan")]
                    crate::native::runtime::texture::Allocation::Vulkan(image) => {
                        image.current_layout.set(image.final_layout)
                    }
                }
                if let Some((id, sync)) = &self.synchronization
                    && id.index() == index
                {
                    match (&texture.state.allocation, sync) {
                        #[cfg(feature = "metal")]
                        (
                            crate::native::runtime::texture::Allocation::Metal(_),
                            crate::native::interop::Synchronization::Metal(_),
                        ) => {}
                        #[cfg(feature = "dx12")]
                        (
                            crate::native::runtime::texture::Allocation::Dx12(allocation),
                            crate::native::interop::Synchronization::Dx12(sync),
                        ) => allocation.state.set(sync.outgoing),
                        #[cfg(feature = "vulkan")]
                        (
                            crate::native::runtime::texture::Allocation::Vulkan(image),
                            crate::native::interop::Synchronization::Vulkan(sync),
                        ) => {
                            image.current_layout.set(sync.outgoing.layout);
                            image.current_family.set(sync.outgoing.queue_family);
                        }
                        #[cfg(all(feature = "dx12", feature = "vulkan"))]
                        _ => unreachable!("validated target synchronization backend"),
                    }
                }
                if written[index] {
                    texture.state.content_version.set(
                        texture
                            .state
                            .content_version
                            .get()
                            .checked_add(1)
                            .expect("native texture content version exhausted"),
                    );
                }
            }
        }
    }
}
