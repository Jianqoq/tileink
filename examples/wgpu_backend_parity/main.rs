mod cases;
#[path = "../common/mod.rs"]
mod common;
mod evidence;
mod gpu;
mod options;
mod pixels;
mod report;
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
    let mut inputs = Vec::new();
    if let Some(input) = &options.input {
        cases::collect_svgs(&input.canonicalize()?, &mut inputs)?;
    }
    inputs.sort();
    if options.input.is_some() && inputs.is_empty() {
        return Err("SVG input contains no cases".into());
    }
    let smoke = if options.input.is_none() {
        cases::smoke_scenes()
    } else {
        Vec::new()
    };
    let case_names: Vec<String> = if options.input.is_some() {
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
    evidence::create_manifest(&options, &case_names, &inputs, &corpus)?;
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
            let route = gpu::create(&instance, backend, *portable, luid.as_deref())?;
            println!("{}", route.metadata);
            luid = Some(route.identity.clone());
            routes.push(route);
        }
    }
    let mut report = report::Report::new(
        &options.output,
        &case_names,
        routes.iter().map(|route| route.metadata.clone()).collect(),
    )?;
    let result = render_cases(&inputs, &corpus.trees, smoke, &mut routes, &mut report)
        .and_then(|()| corpus.verify_unchanged());
    if let Err(error) = result {
        report.fail(&error.to_string())?;
        return Err(error);
    }
    println!("Report: {}", options.output.join("report.json").display());
    report.finish()
}

fn render_cases(
    inputs: &[PathBuf],
    trees: &[usvg::Tree],
    smoke: Vec<(&str, Canvas)>,
    routes: &mut [gpu::Route],
    report: &mut report::Report,
) -> Result<()> {
    for (case, scene) in smoke {
        compare_frame(case, &scene, routes, report)?;
    }
    for (index, (file, tree)) in inputs.iter().zip(trees).enumerate() {
        let (scene, _, _) = common::svg_tree_to_scene(tree, 300)?;
        compare_frame(&cases::svg_case(index, file), &scene, routes, report)?;
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
    report: &mut report::Report,
) -> Result<()> {
    let mut images = Vec::new();
    for route in routes {
        println!("Rendering {case} through {}", route.name);
        route.renderer.render(scene);
        images.push(route.renderer.image());
    }
    report.record(case, &images)?;
    Ok(())
}

#[cfg(all(test, windows))]
mod numeric;
