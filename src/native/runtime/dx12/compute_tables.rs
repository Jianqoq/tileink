use super::super::Result;
use super::super::compute::{Pass, Resource, SamplerFilter};
use crate::native::shaders::BindingKind;
use windows::Win32::Graphics::Direct3D12::*;

#[derive(Clone)]
#[allow(
    clippy::struct_field_names,
    reason = "The wrapper owns a native API handle named after its Vulkan or DX12 object kind"
)]
struct Heap {
    heap: ID3D12DescriptorHeap,
    step: usize,
    next: usize,
    capacity: usize,
}
impl Heap {
    unsafe fn new(
        device: &ID3D12Device,
        kind: D3D12_DESCRIPTOR_HEAP_TYPE,
        count: usize,
        cached: Option<Self>,
    ) -> Result<Option<Self>> {
        let limit = if kind == D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER {
            D3D12_MAX_SHADER_VISIBLE_SAMPLER_HEAP_SIZE as usize
        } else {
            1_000_000
        };
        if count > limit {
            return Err("native DX12 shader-visible heap capacity exceeded".into());
        }
        if let Some(mut cached) = cached
            && cached.capacity >= count
        {
            cached.next = 0;
            return Ok(Some(cached));
        }
        if count == 0 {
            return Ok(None);
        }
        let capacity = count.next_power_of_two().min(limit);
        // SAFETY: The checked descriptor capacity and heap kind form a live-device allocation; the heap is retained by this owner.
        unsafe {
            Ok(Some(Self {
                heap: device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: kind,
                    NumDescriptors: capacity as u32,
                    Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                    NodeMask: 0,
                })?,
                step: device.GetDescriptorHandleIncrementSize(kind) as usize,
                next: 0,
                capacity,
            }))
        }
    }
    unsafe fn visible(&self) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        // SAFETY: The current descriptor index lies within the owned heap capacity and step is the device-reported stride.
        unsafe {
            D3D12_GPU_DESCRIPTOR_HANDLE {
                ptr: self.heap.GetGPUDescriptorHandleForHeapStart().ptr
                    + (self.next * self.step) as u64,
            }
        }
    }
    unsafe fn allocate(&mut self) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        debug_assert!(self.next < self.capacity);
        // SAFETY: The page planner reserves capacity before allocation; the returned CPU handle addresses the next owned heap slot.
        unsafe {
            let handle = D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: self.heap.GetCPUDescriptorHandleForHeapStart().ptr + self.next * self.step,
            };
            self.next += 1;
            handle
        }
    }
}
#[path = "compute_tables/paging.rs"]
mod paging;

#[derive(Clone)]
struct Page {
    resources: Option<Heap>,
    samplers: Option<Heap>,
    end_pass: usize,
}

#[derive(Clone)]
pub(super) struct Tables {
    pages: Vec<Page>,
    current: usize,
}
impl Tables {
    pub(super) fn is_empty(&self) -> bool {
        self.pages
            .iter()
            .all(|page| page.resources.is_none() && page.samplers.is_none())
    }

    pub unsafe fn new(
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        passes: &[Pass],
        cached: Option<Self>,
    ) -> Result<Self> {
        let counts = passes.iter().map(|pass| {
            let mut counts = [0usize; 2];
            for (binding, _) in &pass.bindings {
                let index = usize::from(binding.kind == BindingKind::Sampler);
                counts[index] = counts[index]
                    .checked_add(binding.count as usize)
                    .ok_or("native descriptor count overflow")?;
            }
            Ok(counts)
        });
        let layout = paging::plan(
            counts,
            [
                1_000_000,
                D3D12_MAX_SHADER_VISIBLE_SAMPLER_HEAP_SIZE as usize,
            ],
        )?;
        let mut cached = cached
            .map(|tables| tables.pages)
            .unwrap_or_default()
            .into_iter();
        let mut pages = Vec::with_capacity(layout.len());
        for (end_pass, counts) in layout {
            let previous = cached.next();
            let (resources, samplers) = previous
                .map(|page| (page.resources, page.samplers))
                .unwrap_or_default();
            pages.push(Page {
                // SAFETY: The resource page uses the live device and the capacity reserved by the descriptor planner.
                resources: unsafe {
                    Heap::new(
                        device,
                        D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                        counts[0],
                        resources,
                    )?
                },
                // SAFETY: The sampler page uses the live device and the capacity reserved by the descriptor planner.
                samplers: unsafe {
                    Heap::new(
                        device,
                        D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
                        counts[1],
                        samplers,
                    )?
                },
                end_pass,
            });
        }
        let this = Self { pages, current: 0 };
        if let Some(page) = this.pages.first() {
            // SAFETY: The newly owned page and command list belong to the same device and remain retained by the frame.
            unsafe { page.bind(list) };
        }
        Ok(this)
    }

    pub unsafe fn write(
        &mut self,
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        (pass_index, pass): (usize, &Pass),
        inputs: &[Resource],
        gpu: &super::compute_resources::Resources,
        pipeline: &super::compute_pipeline::Pipeline,
    ) {
        let previous = self.current;
        self.current = self.page_for_pass(pass_index);
        if self.current != previous {
            // A heap switch invalidates root tables. Page::write rebinds every
            // table used by the next pass; earlier pages remain owned until completion.
            // SAFETY: The selected page was reserved for this pass and remains retained while bound to the same-device list.
            unsafe { self.pages[self.current].bind(list) };
        }
        // SAFETY: Validated reflection and resource indices fit this pass page; the frame retains every referenced descriptor resource.
        unsafe { self.pages[self.current].write(device, list, pass, inputs, gpu, pipeline) };
    }

    fn page_for_pass(&self, pass: usize) -> usize {
        // Upload scatter commands can execute before earlier recorded passes.
        // Resolve either direction; pass order and command order are distinct.
        self.pages.partition_point(|page| page.end_pass <= pass)
    }

    #[cfg(test)]
    pub(super) fn heaps(&self) -> Vec<ID3D12DescriptorHeap> {
        self.pages
            .iter()
            .flat_map(|page| [&page.resources, &page.samplers])
            .flatten()
            .map(|heap| heap.heap.clone())
            .collect()
    }
}

impl Page {
    unsafe fn bind(&self, list: &ID3D12GraphicsCommandList) {
        let heaps: Vec<_> = [&self.resources, &self.samplers]
            .into_iter()
            .filter_map(|heap| heap.as_ref().map(|heap| Some(heap.heap.clone())))
            .collect();
        if !heaps.is_empty() {
            // SAFETY: Only live owned heaps from this device are bound; the command list remains live during recording.
            unsafe { list.SetDescriptorHeaps(&heaps) };
        }
    }
    #[allow(
        clippy::expect_used,
        reason = "Reflection allocates the resource/sampler heap before any matching descriptor is written"
    )]
    pub unsafe fn write(
        &mut self,
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        pass: &Pass,
        inputs: &[Resource],
        gpu: &super::compute_resources::Resources,
        pipeline: &super::compute_pipeline::Pipeline,
    ) {
        // SAFETY: Pipeline root slots and page capacities come from validated reflection; every resource and sampler descriptor remains frame-owned.
        unsafe {
            if let Some(root) = pipeline.resources {
                list.SetComputeRootDescriptorTable(
                    root,
                    self.resources
                        .as_ref()
                        .expect("Reflected descriptor bindings have a corresponding allocated heap")
                        .visible(),
                );
            }
            if let Some(root) = pipeline.samplers {
                list.SetComputeRootDescriptorTable(
                    root,
                    self.samplers
                        .as_ref()
                        .expect("Reflected descriptor bindings have a corresponding allocated heap")
                        .visible(),
                );
            }
            for (binding, id) in &pass.bindings {
                let index = id.index();
                if let Resource::TextureTable(images) = &inputs[index] {
                    for image in images {
                        super::compute_bindings::write(
                            device,
                            binding,
                            gpu.get(image.index()),
                            0,
                            0,
                            self.resources.as_mut().expect("Reflected descriptor bindings have a corresponding allocated heap").allocate(),
                        );
                    }
                } else if let Resource::Sampler(filter) = inputs[index] {
                    let handle = self
                        .samplers
                        .as_mut()
                        .expect("Reflected descriptor bindings have a corresponding allocated heap")
                        .allocate();
                    device.CreateSampler(
                        &D3D12_SAMPLER_DESC {
                            Filter: match filter {
                                SamplerFilter::Nearest => D3D12_FILTER_MIN_MAG_MIP_POINT,
                                SamplerFilter::Linear => D3D12_FILTER_MIN_MAG_LINEAR_MIP_POINT,
                            },
                            AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                            AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                            AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                            MaxAnisotropy: 1,
                            ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
                            MinLOD: 0.0,
                            MaxLOD: 0.0,
                            ..Default::default()
                        },
                        handle,
                    );
                } else {
                    super::compute_bindings::write(
                        device,
                        binding,
                        gpu.get(index),
                        gpu.uniform_offset(index).unwrap_or(0),
                        (inputs[index].byte_len() / 4) as u32,
                        self.resources
                            .as_mut()
                            .expect(
                                "Reflected descriptor bindings have a corresponding allocated heap",
                            )
                            .allocate(),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_selection_handles_prepend_scatter_and_skipped_initialization() {
        let tables = Tables {
            pages: [2, 4, 5]
                .map(|end_pass| Page {
                    resources: None,
                    samplers: None,
                    end_pass,
                })
                .to_vec(),
            current: 0,
        };
        // Pass 4 was appended for a scatter upload but executes first; pass 2
        // initializes an already initialized surface and never executes.
        let pages: Vec<_> = [4, 0, 3, 1].map(|pass| tables.page_for_pass(pass)).to_vec();
        assert_eq!(pages, [2, 0, 1, 0]);
    }
}
