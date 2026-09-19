use super::super::Result;
use super::super::compute::{Pass, Resource, SamplerFilter};
use crate::native::shaders::BindingKind;
use windows::Win32::Graphics::Direct3D12::*;

struct Heap {
    heap: ID3D12DescriptorHeap,
    step: usize,
    next: usize,
}
impl Heap {
    unsafe fn new(
        device: &ID3D12Device,
        kind: D3D12_DESCRIPTOR_HEAP_TYPE,
        count: usize,
    ) -> Result<Option<Self>> {
        if count == 0 {
            return Ok(None);
        }
        let limit = if kind == D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER {
            D3D12_MAX_SHADER_VISIBLE_SAMPLER_HEAP_SIZE as usize
        } else {
            1_000_000
        };
        if count > limit {
            return Err("native DX12 shader-visible heap capacity exceeded".into());
        }
        unsafe {
            Ok(Some(Self {
                heap: device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: kind,
                    NumDescriptors: count as u32,
                    Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                    NodeMask: 0,
                })?,
                step: device.GetDescriptorHandleIncrementSize(kind) as usize,
                next: 0,
            }))
        }
    }
    unsafe fn visible(&self) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        unsafe {
            D3D12_GPU_DESCRIPTOR_HANDLE {
                ptr: self.heap.GetGPUDescriptorHandleForHeapStart().ptr
                    + (self.next * self.step) as u64,
            }
        }
    }
    unsafe fn allocate(&mut self) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        unsafe {
            let handle = D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: self.heap.GetCPUDescriptorHandleForHeapStart().ptr + self.next * self.step,
            };
            self.next += 1;
            handle
        }
    }
}
pub(super) struct Tables {
    resources: Option<Heap>,
    samplers: Option<Heap>,
}
impl Tables {
    pub unsafe fn new(
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        passes: &[Pass],
    ) -> Result<Self> {
        let mut counts = [0usize; 2];
        for pass in passes {
            for (binding, _) in &pass.bindings {
                let index = usize::from(binding.kind == BindingKind::Sampler);
                counts[index] = counts[index]
                    .checked_add(binding.count as usize)
                    .ok_or("native descriptor count overflow")?;
            }
        }
        unsafe {
            let this = Self {
                resources: Heap::new(device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, counts[0])?,
                samplers: Heap::new(device, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, counts[1])?,
            };
            let heaps: Vec<_> = [&this.resources, &this.samplers]
                .into_iter()
                .filter_map(|h| h.as_ref().map(|h| Some(h.heap.clone())))
                .collect();
            if !heaps.is_empty() {
                list.SetDescriptorHeaps(&heaps);
            }
            Ok(this)
        }
    }
    pub unsafe fn write(
        &mut self,
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        pass: &Pass,
        inputs: &[Resource],
        gpu: &super::compute_resources::Resources,
        pipeline: &super::compute_pipeline::Pipeline,
    ) {
        unsafe {
            if let Some(root) = pipeline.resources {
                list.SetComputeRootDescriptorTable(
                    root,
                    self.resources.as_ref().unwrap().visible(),
                );
            }
            if let Some(root) = pipeline.samplers {
                list.SetComputeRootDescriptorTable(root, self.samplers.as_ref().unwrap().visible());
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
                            self.resources.as_mut().unwrap().allocate(),
                        );
                    }
                } else if let Resource::Sampler(filter) = inputs[index] {
                    let handle = self.samplers.as_mut().unwrap().allocate();
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
                        self.resources.as_mut().unwrap().allocate(),
                    );
                }
            }
        }
    }
    pub fn into_heaps(self) -> Vec<ID3D12DescriptorHeap> {
        [self.resources, self.samplers]
            .into_iter()
            .flatten()
            .map(|h| h.heap)
            .collect()
    }
}
