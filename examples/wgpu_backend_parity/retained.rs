//! Windows retained route orchestration; core runners and contracts are portable.
use super::retained_contract::Target;
use super::{
    Result,
    common::fonts::Snapshot,
    evidence,
    gpu::Route,
    report::Report,
    retained_sequence::{self, Sequence},
    retained_wgpu::Variant,
};
use serde_json::json;
use std::path::Path;
pub fn render(
    fonts: &Snapshot,
    routes: &[Route],
    native: &super::native::Routes,
    output: &Path,
    validate: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let mut sequence = Sequence::new(fonts)?;
    let names = retained_sequence::names();
    let mut variants = Vec::new();
    let mut metadata = Vec::new();
    for route in routes {
        for kind in [Target::Owned, Target::Transient, Target::Persistent] {
            for full in [false, true] {
                let variant = Variant::new(
                    &route.name,
                    route.renderer.device(),
                    route.renderer.queue(),
                    fonts,
                    kind,
                    full,
                );
                let mut info = route.metadata.clone();
                info["route"] = json!(variant.name);
                info["target"] = json!(kind.name());
                info["incremental_mode"] = json!(if full { "ForceFull" } else { "Auto" });
                metadata.push(info);
                variants.push(variant);
            }
        }
    }
    #[cfg(any(feature = "dx12", feature = "vulkan"))]
    let mut native_variants = native.retained_variants(fonts)?;
    #[cfg(any(feature = "dx12", feature = "vulkan"))]
    metadata.extend(
        native_variants
            .iter()
            .map(|variant| variant.metadata.clone()),
    );
    let mut report = Report::new(output, &names, metadata)?;
    let mut evidence_rows = Vec::new();
    let result: Result<()> = (|| {
        for (&frame, name) in retained_sequence::FRAMES.iter().zip(&names) {
            sequence.apply(frame)?;
            let mut images = Vec::new();
            for variant in &mut variants {
                println!("Rendering {name} through {}", variant.name);
                let (image, row) = variant.render(&sequence, frame)?;
                images.push(image);
                evidence_rows.push(row);
            }
            #[cfg(any(feature = "dx12", feature = "vulkan"))]
            for variant in &mut native_variants {
                println!("Rendering {name} through {}", variant.name);
                let (image, row) = variant.render(&sequence, frame)?;
                images.push(image);
                evidence_rows.push(row);
            }
            report.record(name, &images)?;
            for (variant, image) in variants.iter().zip(&images) {
                variant.validate(&sequence, frame, image)?;
            }
        }
        for (route, variants) in routes.iter().zip(variants.chunks_exact(6)) {
            for variant in variants {
                route
                    .verify_fine_compiler(variant.renderer.precompiled_dxil_pipeline_count() > 0)?;
            }
        }
        Ok(())
    })();
    evidence::write_new_json(
        &output.join("retained-pipelines-and-stats.json"),
        &json!(evidence_rows),
    )?;
    report.finish_checked(
        result
            .and_then(|()| native.validate())
            .and_then(|()| validate()),
    )
}
