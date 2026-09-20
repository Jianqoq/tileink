use super::{Result, common, example_suite, retained_contract::Target, retained_variant::Variant};
use std::rc::Rc;

pub struct Engine {
    context: tileink::NativeContext,
    renderer: tileink::NativeRenderer,
    metadata: serde_json::Value,
}
impl Engine {
    pub fn new(name: &str, luid: &str) -> Result<Self> {
        #[cfg(feature = "dx12")]
        let (expected, backend) = ("native-dx12", tileink::NativeBackend::Dx12);
        #[cfg(feature = "vulkan")]
        let (expected, backend) = ("native-vulkan", tileink::NativeBackend::Vulkan);
        if name != expected {
            return Err("route does not match compiled native backend".into());
        }
        #[cfg(feature = "dx12")]
        unsafe {
            tileink::NativeContext::enable_dx12_validation()?;
        }
        let context = tileink::NativeContext::new(
            backend,
            &tileink::NativeContextOptions {
                physical_adapter: Some(luid.into()),
                validation: true,
            },
        )?;
        let renderer = tileink::NativeRenderer::with_context(&context, 1, 1)?;
        let shaders: Vec<_> = tileink::NATIVE_SHADER_ARTIFACTS.iter().map(|shader| serde_json::json!({
            "entry": shader.entry, "format": shader.format, "cache_key": shader.cache_key,
            "sha256": super::evidence::digest_bytes(shader.bytes), "workgroup": shader.workgroup,
        })).collect();
        let metadata = serde_json::json!({"api":format!("{backend:?}"), "physical_identity":luid,
            "certification_route":name,"validation":true,"shaders":shaders});
        Ok(Self {
            context,
            renderer,
            metadata,
        })
    }
    pub fn metadata(&self) -> serde_json::Value {
        self.metadata.clone()
    }
    pub fn render(&mut self, canvas: &tileink::Canvas) -> Result<tileink::Image> {
        Ok(self.renderer.render_to_image(canvas)?.readback()?)
    }
    pub fn examples(
        &self,
        inputs: Rc<common::capture::Inputs>,
    ) -> Result<common::capture::Captured> {
        common::capture::run_native(
            &self.context,
            inputs,
            example_suite::OUTPUTS,
            example_suite::run,
        )
    }
    pub fn variants(&self, fonts: &common::fonts::Snapshot) -> Result<Vec<Variant>> {
        [Target::Owned, Target::Transient, Target::Persistent]
            .into_iter()
            .flat_map(|kind| {
                [false, true].into_iter().map(move |full| {
                    Variant::new(
                        &self.context,
                        self.metadata["certification_route"].as_str().unwrap(),
                        self.metadata.clone(),
                        fonts,
                        kind,
                        full,
                    )
                })
            })
            .collect()
    }
    pub fn validate_variant(
        &self,
        _: &Variant,
        _: &super::retained_sequence::Sequence,
        _: super::retained_sequence::Frame,
        _: &tileink::Image,
    ) -> Result {
        Ok(())
    }
    pub fn validate(&self, _: bool) -> Result {
        Ok(self.context.check_validation()?)
    }
}
