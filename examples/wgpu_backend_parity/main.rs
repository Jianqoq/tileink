mod cases;
#[path = "../common/mod.rs"]
mod common;
mod evidence;
#[path = "../wgpu/suite.rs"]
mod example_suite;
mod examples;
mod gpu;
#[path = "../common/layer_filter_scenes.rs"]
mod layer_filter_scenes;
mod native;
#[cfg(all(test, windows, any(feature = "dx12", feature = "vulkan")))]
mod native_tests;
mod options;
mod pixels;
mod report;
#[cfg(windows)]
mod retained;
#[cfg(windows)]
mod retained_contract;
mod retained_sequence;
#[cfg(windows)]
mod retained_wgpu;
mod svg;

use std::path::PathBuf;
use tileink::Canvas;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let _ = env_logger::try_init();
    let Some(mut options) = options::Options::parse(std::env::args_os().skip(1))? else {
        println!("{}", options::HELP);
        return Ok(());
    };
    options.resolve_dxc()?;
    native::initialize_validation(options.native)?;
    let mut inputs = Vec::new();
    if let Some(input) = &options.input {
        cases::collect_svgs(&input.canonicalize()?, &mut inputs)?;
    }
    if options.suite == options::Suite::Examples {
        for input in example_suite::SVG_INPUTS {
            inputs.push(std::path::absolute(common::example_asset(input))?);
        }
    }
    inputs.sort();
    if options.input.is_some() && inputs.is_empty() {
        return Err("SVG input contains no cases".into());
    }
    let smoke = if options.input.is_none() && options.suite == options::Suite::Smoke {
        cases::smoke_scenes()
    } else {
        Vec::new()
    };
    let case_names: Vec<String> = if options.suite == options::Suite::Examples {
        example_suite::OUTPUTS
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    } else if options.suite == options::Suite::Retained {
        retained_sequence::names()
    } else if options.input.is_some() {
        inputs
            .iter()
            .enumerate()
            .map(|(index, path)| cases::svg_case(index, path))
            .collect()
    } else {
        smoke.iter().map(|(name, _)| (*name).to_owned()).collect()
    };
    if let Some(parent) = options
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(&options.output)?;
    let corpus = svg::Corpus::load(&inputs)?;
    let example_inputs = if matches!(
        options.suite,
        options::Suite::Examples | options::Suite::Retained
    ) {
        let fonts = common::fonts::Snapshot::system()?;
        fonts.write(&options.output.join("fonts"))?;
        Some(std::rc::Rc::new(common::capture::Inputs {
            fonts,
            svgs: inputs
                .iter()
                .cloned()
                .zip(corpus.trees.iter().cloned())
                .collect(),
        }))
    } else {
        None
    };
    evidence::create_manifest(
        &options,
        &case_names,
        &inputs,
        &corpus,
        example_inputs.as_ref().map(|inputs| &inputs.fonts.manifest),
    )?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
        backend_options: wgpu::BackendOptions {
            dx12: wgpu::Dx12BackendOptions {
                shader_compiler: match &options.dxc {
                    Some(path) => wgpu::Dx12Compiler::DynamicDxc {
                        dxc_path: path
                            .to_str()
                            .ok_or("DXC path is not valid UTF-8")?
                            .to_owned(),
                    },
                    None => wgpu::Dx12Compiler::default(),
                },
                ..Default::default()
            },
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut routes = Vec::new();
    let mut luid = options.luid.clone();
    for portable in &options.textures {
        for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
            let route = gpu::create_with_fine(
                &instance,
                backend,
                *portable,
                luid.as_deref(),
                options.dx12_fine,
            )?;
            println!("{}", route.metadata);
            luid = Some(route.identity.clone());
            routes.push(route);
        }
    }
    let mut native_routes = native::Routes::new(
        options.native,
        luid.as_deref().ok_or("missing physical GPU identity")?,
    )?;
    if options.suite == options::Suite::Retained {
        #[cfg(windows)]
        return retained::render(
            &example_inputs.as_ref().unwrap().fonts,
            &routes,
            &native_routes,
            &options.output,
            || corpus.verify_unchanged(),
        );
        #[cfg(not(windows))]
        return Err("retained cross-API certification requires Windows DX12 and Vulkan".into());
    }
    let mut metadata: Vec<_> = routes.iter().map(|route| route.metadata.clone()).collect();
    metadata.extend(native_routes.metadata());
    let mut report = report::Report::new(&options.output, &case_names, metadata)?;
    let result = if let Some(inputs) = example_inputs {
        examples::render(
            inputs,
            &mut routes,
            &native_routes,
            &mut report,
            &options.output,
        )
    } else {
        render_cases(
            &inputs,
            &corpus.trees,
            smoke,
            &mut routes,
            &mut native_routes,
            &mut report,
        )
        .and_then(|()| {
            for route in &routes {
                route.verify_fine_compiler(route.renderer.precompiled_dxil_pipeline_count() > 0)?;
            }
            Ok(())
        })
    }
    .and_then(|()| corpus.verify_unchanged())
    .and_then(|()| native_routes.validate());
    if let Err(error) = result {
        report.fail(&error.to_string())?;
        return Err(error);
    }
    evidence::write_new_json(
        &options.output.join("route-pipelines.json"),
        &serde_json::json!(routes.iter().map(|route|serde_json::json!({
        "route":route.name,"compiled_pipelines":route.renderer.pipeline_compilation_epoch(),
        "precompiled_dxil_pipelines":route.renderer.precompiled_dxil_pipeline_count(),
    })).collect::<Vec<_>>()),
    )?;
    println!("Report: {}", options.output.join("report.json").display());
    report.finish()
}

fn render_cases(
    inputs: &[PathBuf],
    trees: &[usvg::Tree],
    smoke: Vec<(&str, Canvas)>,
    routes: &mut [gpu::Route],
    native_routes: &mut native::Routes,
    report: &mut report::Report,
) -> Result<()> {
    for (case, scene) in smoke {
        compare_frame(case, &scene, routes, native_routes, report)?;
    }
    for (index, (file, tree)) in inputs.iter().zip(trees).enumerate() {
        let (scene, _, _) = common::svg_tree_to_scene(tree, 300)?;
        compare_frame(
            &cases::svg_case(index, file),
            &scene,
            routes,
            native_routes,
            report,
        )?;
        if (index + 1) % 25 == 0 {
            println!("Compared {} / {} SVGs", index + 1, inputs.len());
        }
    }
    Ok(())
}

fn compare_frame(
    case: &str,
    scene: &Canvas,
    routes: &mut [gpu::Route],
    native_routes: &mut native::Routes,
    report: &mut report::Report,
) -> Result<()> {
    let mut images = Vec::new();
    for route in routes {
        println!("Rendering {case} through {}", route.name);
        route.renderer.render(scene);
        images.push(route.renderer.image());
    }
    native_routes.render(scene, &mut images)?;
    report.record(case, &images)?;
    Ok(())
}

#[cfg(all(test, windows))]
mod numeric;

#[cfg(all(test, windows))]
mod filter_sequence;

#[cfg(all(test, windows))]
mod coverage;

#[cfg(all(test, windows))]
mod probe;

#[cfg(all(test, windows))]
mod gradient;

#[cfg(all(test, windows))]
mod filter_numeric;

#[cfg(all(test, windows))]
mod lighting;

#[cfg(all(test, windows))]
mod raster;

#[cfg(any(test, windows))]
mod readback;

#[cfg(all(test, windows))]
mod blur_composite;

#[cfg(all(test, windows))]
mod blur_boundaries;

#[cfg(all(test, windows))]
mod glass_refraction;

#[cfg(all(test, windows))]
mod glass_dispersion;
