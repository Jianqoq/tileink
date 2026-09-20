use super::{
    Result, common, example_suite, gpu, options, retained_contract::Target,
    retained_variant::Variant,
};
use std::rc::Rc;

pub struct Engine {
    pub route: gpu::Route,
}

impl Engine {
    pub fn new(name: &str, luid: &str) -> Result<Self> {
        let (api, portable, fine) = match name {
            "wgpu-dx12-native" => (wgpu::Backend::Dx12, false, options::Dx12Fine::Runtime),
            "wgpu-dx12-portable" => (wgpu::Backend::Dx12, true, options::Dx12Fine::Runtime),
            "wgpu-dx12-precompiled" => (wgpu::Backend::Dx12, true, options::Dx12Fine::Precompiled),
            "wgpu-vulkan-native" => (wgpu::Backend::Vulkan, false, options::Dx12Fine::Runtime),
            "wgpu-vulkan-portable" => (wgpu::Backend::Vulkan, true, options::Dx12Fine::Runtime),
            _ => return Err("route does not match compiled wgpu backend".into()),
        };
        let compiler = std::path::PathBuf::from(
            std::env::var_os("TILEINK_PARITY_DXCOMPILER").ok_or("missing pinned DXC library")?,
        )
        .canonicalize()?;
        if api == wgpu::Backend::Dx12 {
            // wgpu silently proceeds without validation when Graphics Tools is
            // missing. Require the interface before certifying this route.
            let mut debug: Option<windows::Win32::Graphics::Direct3D12::ID3D12Debug> = None;
            // SAFETY: only queries the debug interface; no device or global state changes.
            unsafe {
                windows::Win32::Graphics::Direct3D12::D3D12GetDebugInterface(&mut debug)?;
            }
            debug.ok_or("DX12 acceptance requires the D3D12 debug interface")?;
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            // DX12 DEBUG disables DXC optimization; Vulkan DEBUG installs the
            // validation messenger without changing shader optimization.
            flags: if api == wgpu::Backend::Dx12 {
                wgpu::InstanceFlags::VALIDATION
            } else {
                wgpu::InstanceFlags::VALIDATION | wgpu::InstanceFlags::DEBUG
            },
            backends: if api == wgpu::Backend::Dx12 {
                wgpu::Backends::DX12
            } else {
                wgpu::Backends::VULKAN
            },
            backend_options: wgpu::BackendOptions {
                dx12: wgpu::Dx12BackendOptions {
                    shader_compiler: wgpu::Dx12Compiler::DynamicDxc {
                        dxc_path: compiler
                            .to_str()
                            .ok_or("DXC library path is not UTF-8")?
                            .into(),
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let mut route = gpu::create_with_fine(&instance, api, portable, Some(luid), fine)?;
        route.metadata["compiler_library_sha256"] =
            serde_json::json!(super::evidence::digest_file(&compiler)?);
        route.metadata["certification_route"] = serde_json::json!(name);
        route.metadata["validation"] = serde_json::json!(true);
        route.metadata["shader_debug"] = serde_json::json!(api != wgpu::Backend::Dx12);
        Ok(Self { route })
    }
    pub fn metadata(&self) -> serde_json::Value {
        self.route.metadata.clone()
    }
    pub fn render(&mut self, canvas: &tileink::Canvas) -> Result<tileink::Image> {
        self.route.renderer.render(canvas);
        Ok(self.route.renderer.image())
    }
    pub fn examples(
        &self,
        inputs: Rc<common::capture::Inputs>,
    ) -> Result<common::capture::Captured> {
        let result = common::capture::run(
            self.route.renderer.device(),
            self.route.renderer.queue(),
            inputs,
            example_suite::OUTPUTS,
            example_suite::run,
        )?;
        self.route
            .verify_fine_compiler(result.precompiled_dxil_seen)?;
        Ok(result)
    }
    pub fn variants(&self, fonts: &common::fonts::Snapshot) -> Result<Vec<Variant>> {
        Ok([Target::Owned, Target::Transient, Target::Persistent]
            .into_iter()
            .flat_map(|kind| {
                [false, true].into_iter().map(move |full| {
                    Variant::new(
                        &self.route.name,
                        self.route.renderer.device(),
                        self.route.renderer.queue(),
                        fonts,
                        kind,
                        full,
                    )
                })
            })
            .collect())
    }
    pub fn validate_variant(
        &self,
        variant: &Variant,
        sequence: &super::retained_sequence::Sequence,
        frame: super::retained_sequence::Frame,
        image: &tileink::Image,
    ) -> Result {
        variant.validate(sequence, frame, image)?;
        self.route
            .verify_fine_compiler(variant.renderer.precompiled_dxil_pipeline_count() > 0)
    }
    pub fn validate(&self, immediate: bool) -> Result {
        if immediate {
            self.route
                .verify_fine_compiler(self.route.renderer.precompiled_dxil_pipeline_count() > 0)?;
        }
        Ok(())
    }
}
