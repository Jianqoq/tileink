use sha2::{Digest, Sha256};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

#[derive(Clone)]
pub(super) struct Compiled {
    pub pipeline: wgpu::ComputePipeline,
    pub layouts: Vec<wgpu::BindGroupLayout>,
}
#[derive(Eq, Ord, PartialEq, PartialOrd)]
struct PipelineKey {
    source: [u8; 32],
    entry: String,
    layout: String,
}

// Device-local reference objects. Keys include the complete production source,
// entry point, binding types/counts and group partition. Case data is not cached.
#[derive(Default)]
pub(super) struct ComputeCache {
    entries: RefCell<BTreeMap<PipelineKey, Compiled>>,
    builds: Cell<usize>,
}
impl ComputeCache {
    pub fn builds(&self) -> usize {
        self.builds.get()
    }
    pub fn pipeline(
        &self,
        device: &wgpu::Device,
        source: &str,
        entry: &str,
        entries: &[wgpu::BindGroupLayoutEntry],
        table_slots: &[u32],
    ) -> Compiled {
        let key = PipelineKey {
            source: Sha256::digest(source).into(),
            entry: entry.to_owned(),
            layout: format!("{entries:?}/{table_slots:?}"),
        };
        if let Some(compiled) = self.entries.borrow().get(&key) {
            return compiled.clone();
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("production WGSL compute reference"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let group_count = if table_slots.is_empty() { 1 } else { 2 };
        let layouts: Vec<_> = (0..group_count)
            .map(|group| {
                let entries: Vec<_> = entries
                    .iter()
                    .copied()
                    .filter(|e| usize::from(table_slots.contains(&e.binding)) == group)
                    .collect();
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
                    entries: &entries,
                })
            })
            .collect();
        let refs: Vec<_> = layouts.iter().map(Some).collect();
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &refs,
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        });
        self.builds.set(self.builds.get() + 1);
        let compiled = Compiled { pipeline, layouts };
        self.entries.borrow_mut().insert(key, compiled.clone());
        compiled
    }
}
