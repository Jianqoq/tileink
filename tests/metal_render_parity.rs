//! Separate executables certify production renderers on the same physical GPU.
#![cfg(all(target_os = "macos", any(feature = "wgpu", feature = "metal")))]
#[cfg(feature = "wgpu")]
#[allow(dead_code)]
#[path = "../examples/common/benchmark_gpu.rs"]
mod gpu;
use peniko::{
    Color, Gradient,
    kurbo::{Affine, BezPath, Circle, Rect, Shape},
};
use tileink::{Canvas, FillRule, Radius};

fn scenes() -> Vec<(&'static str, Canvas)> {
    let mut scenes = Vec::new();
    let mut rects = Canvas::new(67, 49, 1.0);
    rects.push_rect(
        Rect::new(0., 0., 67., 49.),
        Radius::ZERO,
        Color::from_rgb8(19, 37, 61),
    );
    rects.push_rect(
        Rect::new(3., 7., 41., 33.),
        Radius::ZERO,
        Color::from_rgba8(230, 90, 15, 127),
    );
    rects.push_rect(
        Rect::new(29.25, -2.5, 70.5, 40.75),
        Radius::all(8.),
        Color::from_rgba8(40, 210, 130, 191),
    );
    rects.push_circle(
        Circle::new((29.5, 29.5), 16.25),
        Color::from_rgba8(200, 30, 70, 128),
    );
    scenes.push(("sdf-overlap", rects));
    let mut paths = Canvas::new(67, 49, 1.0);
    let mut path = BezPath::new();
    path.move_to((-7.25, 12.5));
    path.curve_to((50.5, -27.), (12., 63.), (71.25, 24.5));
    path.line_to((5.5, 51.25));
    path.close_path();
    paths.push_path(
        path.clone(),
        Color::from_rgba8(151, 241, 71, 217),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    paths.push_path(
        path,
        Color::from_rgba8(211, 41, 171, 113),
        Affine::translate((4.25, -3.5)),
        FillRule::EvenOdd,
        0.1,
    );
    scenes.push(("curved-path", paths));
    let mut gradients = Canvas::new(67, 49, 1.0);
    let colors = [
        Color::from_rgba8(230, 50, 90, 217),
        Color::from_rgba8(20, 210, 130, 97),
        Color::from_rgb8(40, 70, 210),
    ];
    let linear = Gradient::new_linear((3.25, 9.5), (60.75, 31.25)).with_stops(colors);
    gradients.push_rect(Rect::new(-2., 1.25, 65.25, 45.5), Radius::all(7.), &linear);
    let radial = Gradient::new_radial((34.25, 23.5), 23.75).with_stops(colors);
    gradients.push_circle(Circle::new((34.25, 23.5), 22.75), &radial);
    scenes.push(("gradients", gradients));
    let mut clips = Canvas::new(67, 49, 1.0);
    for i in 0..7 {
        clips.push_clip_sdf_rect_layer(
            Rect::new(i as f64 + 0.25, 1.5, 64.25, 47.75),
            Radius::all(8.),
        );
    }
    clips.push_clip_layer(
        Circle::new((34., 24.), 22.75).to_path(0.1),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    clips.push_rect(
        Rect::new(0., 0., 67., 49.),
        Radius::ZERO,
        Color::from_rgba8(230, 71, 191, 173),
    );
    for _ in 0..8 {
        clips.pop_layer();
    }
    scenes.push(("clip-spills", clips));
    let mut groups = Canvas::new(67, 49, 1.0);
    groups.push_rect(
        Rect::new(0., 0., 67., 49.),
        Radius::ZERO,
        Color::from_rgba8(10, 50, 200, 180),
    );
    for i in 0..5 {
        groups.push_opacity_layer(
            Circle::new((33., 24.), 24.5 - i as f64).to_path(0.1),
            Affine::IDENTITY,
            0.1,
            0.83,
        );
    }
    groups.push_rect(
        Rect::new(1.5, 3.25, 65.5, 47.75),
        Radius::all(5.),
        Color::from_rgba8(241, 97, 11, 213),
    );
    for _ in 0..5 {
        groups.pop_layer();
    }
    scenes.push(("group-spills", groups));
    for (name, mix) in [
        ("blend-multiply", peniko::Mix::Multiply),
        ("blend-dodge", peniko::Mix::ColorDodge),
        ("blend-hue", peniko::Mix::Hue),
        ("blend-softlight", peniko::Mix::SoftLight),
    ] {
        let mut scene = Canvas::new(67, 49, 1.0);
        scene.push_circle(
            Circle::new((24.5, 24.), 23.),
            Color::from_rgba8(70, 130, 210, 193),
        );
        scene.push_blend_layer(
            Rect::new(0., 0., 67., 49.).to_path(0.1),
            Affine::IDENTITY,
            0.1,
            mix,
            peniko::Compose::SrcOver,
        );
        scene.push_circle(
            Circle::new((42.5, 24.), 23.),
            Color::from_rgba8(211, 73, 143, 177),
        );
        scene.pop_layer();
        scenes.push((name, scene));
    }
    let mut images = Canvas::new(67, 49, 1.0);
    let bytes: Vec<u8> = (0..35u32)
        .flat_map(|i| {
            [
                (i * 17) as u8,
                (i * 37) as u8,
                (i * 71) as u8,
                (i * 43) as u8,
            ]
        })
        .collect();
    let image = std::rc::Rc::new(tileink::Image::from_rgba8(7, 5, bytes));
    images.push_image(
        Rect::new(-3.25, 1.75, 33.5, 42.5),
        image.clone(),
        peniko::Extend::Pad,
        tileink::PatternSampling::Nearest,
    );
    images.push_image(
        Rect::new(29.25, 4.75, 72.5, 51.5),
        image,
        peniko::Extend::Pad,
        tileink::PatternSampling::Bilinear,
    );
    scenes.push(("images", images));
    for (name, deviation) in [("blur-small", 1.25), ("blur-large", 9.75)] {
        let mut scene = Canvas::new(67, 49, 1.0);
        scene.push_filter_layer(
            tileink::Filter::Blur {
                std_dev_x: deviation,
                std_dev_y: deviation * 0.7,
                sampling: Default::default(),
            },
            tileink::Region::rect(Rect::new(1.25, 1.75, 65.5, 47.25), Radius::all(4.5)),
        );
        scene.push_circle(
            Circle::new((23.75, 20.5), 13.25),
            Color::from_rgba8(213, 71, 151, 197),
        );
        scene.push_rect(
            Rect::new(29.5, 11.25, 57.75, 42.5),
            Radius::all(3.25),
            Color::from_rgba8(31, 171, 97, 127),
        );
        scene.pop_layer();
        scenes.push((name, scene));
    }
    scenes
}

#[test]
#[ignore = "run scripts/mac/run_native_metal_tests.sh for fresh same-device reference"]
fn production_render_parity() -> Result<(), Box<dyn std::error::Error>> {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/metal-validation/render");
    std::fs::create_dir_all(&directory)?;
    #[cfg(feature = "wgpu")]
    let (_, device, queue) = {
        let result = gpu::device("metal", false, false, wgpu::MemoryHints::MemoryUsage);
        std::fs::write(
            directory.join("identity.json"),
            serde_json::to_vec_pretty(&result.0)?,
        )?;
        result
    };
    #[cfg(feature = "metal")]
    let context = {
        let identity: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("identity.json"))?)?;
        tileink::NativeContext::new(
            tileink::NativeBackend::Metal,
            &tileink::NativeContextOptions {
                physical_adapter: Some(
                    identity["physical_identity"]
                        .as_str()
                        .ok_or("missing physical identity")?
                        .into(),
                ),
                validation: true,
            },
        )?
    };
    for (name, canvas) in scenes() {
        let (width, height) = canvas.physical_size();
        #[cfg(feature = "wgpu")]
        {
            let mut renderer =
                tileink::WgpuRenderer::new(&device, &queue, width, height, Color::TRANSPARENT);
            renderer.render(&canvas);
            std::fs::write(
                directory.join(format!("{name}.bin")),
                bytemuck::cast_slice(&renderer.image().pixels),
            )?;
        }
        #[cfg(feature = "metal")]
        {
            let mut renderer = tileink::NativeRenderer::with_context(&context, width, height)?;
            renderer.set_clear_color(Color::TRANSPARENT);
            for repeat in 0..3 {
                let image = renderer.render_to_image(&canvas)?.readback()?;
                let actual: &[u8] = bytemuck::cast_slice(&image.pixels);
                let expected = std::fs::read(directory.join(format!("{name}.bin")))?;
                let diff = actual.iter().zip(&expected).position(|(a, b)| a != b);
                if let Some(index) = diff {
                    std::fs::write(directory.join(format!("{name}-native.bin")), actual)?;
                    return Err(format!("{name} repeat {repeat}: pixel ({}, {}), actual {:?}, expected {:?}, {} differing bytes", index/4%width as usize,index/4/width as usize,&actual[index/4*4..index/4*4+4],&expected[index/4*4..index/4*4+4],actual.iter().zip(&expected).filter(|(a,b)|a!=b).count()).into());
                }
                assert_eq!(actual.len(), expected.len());
            }
        }
    }
    #[cfg(feature = "metal")]
    context.check_validation()?;
    Ok(())
}
