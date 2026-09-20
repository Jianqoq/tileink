use super::{
    Result, common,
    engine::Engine,
    evidence, example_suite,
    report::Report,
    retained_sequence::{FRAMES, Sequence},
    svg,
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

fn collect(directory: &Path, result: &mut Vec<PathBuf>) -> Result {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, result)?;
        } else if path.extension().is_some_and(|ext| ext == "svg") {
            result.push(path);
        }
    }
    Ok(())
}

fn retained_frames() -> impl Iterator<Item = (String, super::retained_sequence::Frame)> {
    // The shared manifest includes the sequence index, not just Frame::name().
    // Use its IDs for recording so the report validates the actual declared cases.
    super::retained_sequence::names()
        .into_iter()
        .zip(FRAMES.iter().copied())
}

#[test]
fn retained_frame_outputs_complete_the_declared_manifest() -> Result {
    let directory = tempfile::tempdir()?;
    let mut report = Report::new(
        directory.path(),
        &super::retained_sequence::names(),
        6,
        json!({}),
    )?;
    let image = tileink::Image {
        width: 1,
        height: 1,
        pixels: vec![0],
    };
    for (case, _) in retained_frames() {
        for variant in 0..6 {
            report.record(&case, variant, &image, json!({}))?;
        }
    }
    report.finish(Ok(()))
}

pub fn run() -> Result {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = PathBuf::from(
        std::env::var_os("TILEINK_ACCEPTANCE_OUTPUT").ok_or("missing output directory")?,
    );
    let route = std::env::var("TILEINK_ACCEPTANCE_ROUTE")?;
    let luid = std::env::var("TILEINK_ACCEPTANCE_GPU")?.to_ascii_lowercase();
    if luid.len() != 16 || !luid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("expected 16 hexadecimal LUID bytes".into());
    }
    let suite = std::env::var("TILEINK_ACCEPTANCE_SUITE")?;
    if !matches!(suite.as_str(), "svg" | "examples" | "retained" | "rounding") {
        return Err("unknown acceptance suite".into());
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(&output)?;
    let sources = evidence::source_snapshot(&[], &output)?;
    let mut inputs = Vec::new();
    if suite == "svg" {
        collect(&root.join("src/svg/tests"), &mut inputs)?;
    } else if suite == "rounding" {
        // These production shaders exposed optimizer-dependent product rounding.
        // Compare complete images on the same GPU; lighting has no cross-device golden.
        inputs.extend([
            root.join("src/svg/tests/filters/feTurbulence/baseFrequency=0.05-0.01.svg"),
            root.join("src/svg/tests/filters/feSpecularLighting/with-feSpotLight.svg"),
        ]);
    } else if suite == "examples" {
        inputs.extend(
            example_suite::SVG_INPUTS
                .iter()
                .map(|path| common::example_asset(path)),
        );
    }
    inputs.sort();
    let corpus = svg::Corpus::load(&inputs)?;
    let cases: Vec<String> = match suite.as_str() {
        "svg" | "rounding" => inputs
            .iter()
            .map(|path| {
                path.strip_prefix(root.join("src/svg/tests"))
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect(),
        "examples" => example_suite::OUTPUTS
            .iter()
            .map(|name| (*name).into())
            .collect(),
        _ => super::retained_sequence::names(),
    };
    let fonts = if !matches!(suite.as_str(), "svg" | "rounding") {
        Some(common::fonts::Snapshot::system()?)
    } else {
        None
    };
    let mut engine = Engine::new(&route, &luid)?;
    let mut report = Report::new(
        &output,
        &cases,
        if suite == "retained" { 6 } else { 1 },
        json!({
            "route":route,"suite":suite,"physical_identity":luid,"device":engine.metadata(),
            "executable_sha256":evidence::digest_file(&std::env::current_exe()?)?,
            "sources":sources,"resources":corpus.snapshot,"fonts":fonts.as_ref().map(|fonts| &fonts.manifest),
        }),
    )?;
    let result = (|| {
        match suite.as_str() {
            "svg" | "rounding" => {
                for (case, tree) in cases.iter().zip(&corpus.trees) {
                    let size = tree
                        .size()
                        .to_int_size()
                        .scale_to_width(300)
                        .ok_or("invalid SVG size")?;
                    let mut canvas = tileink::Canvas::new(size.width(), size.height(), 1.0);
                    canvas.push_svg_with_options(
                        tree,
                        tileink::SvgOptions {
                            transform: peniko::kurbo::Affine::scale_non_uniform(
                                size.width() as f64 / tree.size().width() as f64,
                                size.height() as f64 / tree.size().height() as f64,
                            ),
                            ..Default::default()
                        },
                    )?;
                    report.record(case, 0, &engine.render(&canvas)?, json!({}))?;
                }
            }
            "examples" => {
                let captured = engine.examples(Rc::new(common::capture::Inputs {
                    fonts: fonts.unwrap(),
                    svgs: inputs
                        .iter()
                        .cloned()
                        .zip(corpus.trees.iter().cloned())
                        .collect(),
                }))?;
                for (name, image) in captured.images {
                    report.record(&name, 0, &image, json!({"pipelines":captured.pipelines}))?;
                }
            }
            _ => {
                let fonts = fonts.unwrap();
                let mut sequence = Sequence::new(&fonts)?;
                let mut variants = engine.variants(&fonts)?;
                for (case, frame) in retained_frames() {
                    sequence.apply(frame)?;
                    for (index, variant) in variants.iter_mut().enumerate() {
                        let (image, details) = variant.render(&sequence, frame)?;
                        engine.validate_variant(variant, &sequence, frame, &image)?;
                        report.record(&case, index, &image, details)?;
                    }
                }
            }
        }
        engine.validate(matches!(suite.as_str(), "svg" | "rounding"))?;
        corpus.verify_unchanged()?;
        if evidence::source_snapshot(&[], &output)? != sources {
            return Err("sources changed during acceptance run".into());
        }
        Ok(())
    })();
    report.finish(result)
}
