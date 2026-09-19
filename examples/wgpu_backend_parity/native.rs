//! Optional owned native routes for the same immediate corpus and raw comparator.
use super::{Result, common::capture};
use std::rc::Rc;
use tileink::{Canvas, Image};

#[derive(Default)]
pub struct Routes {
    #[cfg(all(windows, feature = "native"))]
    routes: Vec<Route>,
}

#[cfg(all(windows, feature = "native"))]
struct Route {
    name: String,
    renderer: tileink::NativeRenderer,
    metadata: serde_json::Value,
}

pub fn initialize_validation(enabled: bool) -> Result<()> {
    if !enabled {
        return Ok(());
    }
    #[cfg(all(windows, feature = "native"))]
    {
        // SAFETY: main calls this before any wgpu instance/device or native context.
        // This standalone verifier has no concurrent external device creation.
        unsafe {
            tileink::NativeContext::enable_dx12_validation()?;
        }
        Ok(())
    }
    #[cfg(not(all(windows, feature = "native")))]
    Err("--native requires Windows and --features native".into())
}

impl Routes {
    pub fn new(enabled: bool, luid: &str) -> Result<Self> {
        if !enabled {
            return Ok(Self::default());
        }
        #[cfg(all(windows, feature = "native"))]
        {
            let mut routes = Vec::new();
            for (backend, name) in [
                (tileink::NativeBackend::Dx12, "native-dx12"),
                (tileink::NativeBackend::Vulkan, "native-vulkan"),
            ] {
                let context = tileink::NativeContext::new(
                    backend,
                    &tileink::NativeContextOptions {
                        physical_adapter: Some(luid.to_owned()),
                        validation: true,
                    },
                )?;
                routes.push(Route {
                    name: name.to_owned(),
                    renderer: tileink::NativeRenderer::with_context(&context, 1, 1)?,
                    metadata: serde_json::json!({"route": name, "api": format!("{backend:?}"),
                        "implementation": "native", "shader_language": "HLSL", "physical_identity": luid,
                        "validation": true, "shader_artifacts": if backend == tileink::NativeBackend::Dx12 { "DXIL" } else { "SPIR-V" }}),
                });
            }
            Ok(Self { routes })
        }
        #[cfg(not(all(windows, feature = "native")))]
        {
            let _ = luid;
            Err("--native requires Windows and --features native".into())
        }
    }

    pub fn metadata(&self) -> Vec<serde_json::Value> {
        #[cfg(all(windows, feature = "native"))]
        {
            self.routes
                .iter()
                .map(|route| route.metadata.clone())
                .collect()
        }
        #[cfg(not(all(windows, feature = "native")))]
        {
            Vec::new()
        }
    }

    pub fn render(&mut self, canvas: &Canvas, images: &mut Vec<Image>) -> Result<()> {
        #[cfg(all(windows, feature = "native"))]
        for route in &mut self.routes {
            println!("Rendering through {}", route.name);
            images.push(route.renderer.render_to_image(canvas)?.readback()?);
        }
        #[cfg(not(all(windows, feature = "native")))]
        let _ = (canvas, images);
        Ok(())
    }

    pub fn examples(
        &self,
        inputs: Rc<capture::Inputs>,
    ) -> Result<Vec<(String, capture::Captured)>> {
        #[cfg(all(windows, feature = "native"))]
        {
            self.routes
                .iter()
                .map(|route| {
                    println!("Rendering all examples through {}", route.name);
                    Ok((
                        route.name.clone(),
                        capture::run_native(
                            route.renderer.context(),
                            inputs.clone(),
                            super::example_suite::OUTPUTS,
                            super::example_suite::run,
                        )?,
                    ))
                })
                .collect()
        }
        #[cfg(not(all(windows, feature = "native")))]
        {
            let _ = inputs;
            Ok(Vec::new())
        }
    }

    pub fn validate(&self) -> Result<()> {
        #[cfg(all(windows, feature = "native"))]
        for route in &self.routes {
            route.renderer.context().check_validation()?;
        }
        Ok(())
    }
}
