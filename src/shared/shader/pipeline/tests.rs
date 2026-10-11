use super::*;
fn kernel(bindings: &[(u32, ShaderBindingKind)]) -> ShaderKernel {
    ShaderKernel::new(
        ShaderSources {
            metal: Some("native source".into()),
            hlsl: None,
        },
        "main",
        [8, 8, 1],
        bindings
            .iter()
            .map(|&(slot, kind)| ShaderBinding {
                name: format!("r{slot}"),
                slot,
                kind,
            })
            .collect(),
    )
    .unwrap()
}
fn pass(bindings: &[(u32, ShaderBindingKind, usize)]) -> ShaderPass {
    ShaderPass {
        kernel: kernel(&bindings.iter().map(|&(s, k, _)| (s, k)).collect::<Vec<_>>()),
        bindings: bindings.iter().map(|&(s, _, r)| (s, r)).collect(),
        dispatch: ShaderDispatch::Image(ShaderExtent::FULL),
    }
}
const F: ShaderTextureFormat = ShaderTextureFormat::Rgba16Float;
#[test]
fn indexed_inputs_validate_and_output_growth_is_independent_of_read_radius() {
    let make = |inputs: Vec<_>| {
        let mut resources = inputs;
        resources.push(ShaderResource::Output);
        ShaderPipeline::new(
            resources,
            vec![pass(&[
                (1, ShaderBindingKind::Texture, 0),
                (2, ShaderBindingKind::Texture, 1),
                (
                    3,
                    ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
                    2,
                ),
            ])],
        )
    };
    assert!(make(vec![ShaderResource::Input(0), ShaderResource::Input(0)]).is_err());
    assert!(make(vec![ShaderResource::Input(0), ShaderResource::Input(2)]).is_err());
    assert!(
        make(vec![
            ShaderResource::Input(usize::MAX),
            ShaderResource::Input(0)
        ])
        .is_err()
    );
    let p = make(vec![ShaderResource::Input(1), ShaderResource::Input(0)]).unwrap();
    assert_eq!(p.input_count(), 2);
    assert!(p.requires_output_scratch());
    assert_eq!(p.output_outset(), 0);
    let mut p = p
        .with_output_outset(3.25)
        .unwrap()
        .with_input_radius(2.0)
        .unwrap();
    assert_eq!(p.output_outset(), 4);
    assert_eq!(p.sample_outset(), Some(3));
    p.transform = [0.5, 0.0, 0.0, 0.25, 12.0, -9.0];
    assert_eq!(p.output_outset(), 13);
    assert_eq!(p.sample_outset(), Some(9));
    assert_ne!(
        format!("{p:?}"),
        format!("{:?}", p.with_output_outset(4.0).unwrap())
    );
    for v in [-1.0, f32::NAN, f32::INFINITY] {
        assert!(p.with_output_outset(v).is_err());
    }
    let region = crate::Region::rect(
        peniko::kurbo::Rect::new(10.0, 10.0, 20.0, 20.0),
        crate::Radius::ZERO,
    );
    let p = p.with_output_outset(0.5).unwrap();
    assert_eq!(
        crate::shared::layer::filter::unclipped_filtered_region_bounds(&p.into(), &region),
        crate::Bounds::new(8, 8, 22, 22)
    );
}

#[test]
fn chained_output_growth_saturates_before_clipping_instead_of_wrapping() {
    let p = ShaderPipeline::new(
        vec![ShaderResource::Output],
        vec![pass(&[(
            3,
            ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
            0,
        )])],
    )
    .unwrap()
    .with_output_outset(f32::MAX)
    .unwrap();
    let chain = crate::Filter::Chain {
        filters: vec![p.clone().into(), p.into()],
        fixed_region: false,
    };
    assert_eq!(
        crate::shared::layer::filter::filter_outset(&chain),
        i32::MAX
    );
    let canvas = crate::Bounds::canvas(16, 16);
    let region = crate::Region::rect(
        peniko::kurbo::Rect::new(4.0, 4.0, 8.0, 8.0),
        crate::Radius::ZERO,
    );
    assert_eq!(
        crate::shared::layer::filter::filtered_region_bounds(&chain, &region, canvas),
        canvas
    );
}
#[test]
fn extent_rounds_up_odd_dimensions_and_precision_tracks_bytes() {
    assert_eq!(ShaderExtent::downsample(2).resolve([17, 9]), [9, 5]);
    assert_eq!(ShaderExtent::Fixed([7, 3]).resolve([17, 9]), [7, 3]);
    assert_eq!(
        [
            ShaderTextureFormat::Rgba8Unorm,
            F,
            ShaderTextureFormat::Rgba32Float
        ]
        .map(|f| f.bytes_per_pixel()),
        [4, 8, 16]
    );
}
#[test]
fn pipeline_validates_lifetimes_formats_aliases_and_snapshot_updates() {
    let resources = vec![
        ShaderResource::Input(0),
        ShaderResource::Texture {
            extent: ShaderExtent::downsample(2),
            format: F,
        },
        ShaderResource::Output,
        ShaderResource::Buffer(Rc::from([1])),
    ];
    let write = pass(&[
        (1, ShaderBindingKind::Texture, 0),
        (2, ShaderBindingKind::WriteTexture(F), 1),
    ]);
    let finish = pass(&[
        (1, ShaderBindingKind::Texture, 1),
        (
            2,
            ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
            2,
        ),
        (3, ShaderBindingKind::ReadWriteBuffer, 3),
    ]);
    let pipeline =
        ShaderPipeline::new(resources.clone(), vec![write.clone(), finish.clone()]).unwrap();
    assert!(pipeline.reads_input());
    assert!(ShaderPipeline::new(resources.clone(), vec![finish.clone(), write.clone()]).is_err());
    assert!(
        pipeline
            .with_resource(
                1,
                ShaderResource::Texture {
                    extent: ShaderExtent::FULL,
                    format: ShaderTextureFormat::Rgba32Float
                }
            )
            .is_err()
    );
    assert!(
        pipeline
            .with_resource(3, ShaderResource::Buffer(Rc::from([5])))
            .is_ok()
    );
    assert!(pipeline.with_resource(4, ShaderResource::Input(0)).is_err());
    let mut aliased = finish;
    aliased.bindings[2].1 = 1;
    assert!(ShaderPipeline::new(resources.clone(), vec![write.clone(), aliased]).is_err());
    assert!(ShaderPipeline::new(resources, vec![write]).is_err());
}
#[test]
fn invalid_resource_descriptors_and_dispatches_are_rejected() {
    for e in [ShaderExtent::downsample(0), ShaderExtent::Fixed([0, 1])] {
        assert!(
            ShaderPipeline::new(
                vec![
                    ShaderResource::Output,
                    ShaderResource::Texture {
                        extent: e,
                        format: F
                    }
                ],
                vec![pass(&[(
                    1,
                    ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
                    0
                )])]
            )
            .is_err()
        );
    }
    let mut p = pass(&[(
        1,
        ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
        0,
    )]);
    p.dispatch = ShaderDispatch::Groups([65536, 1, 1]);
    assert!(ShaderPipeline::new(vec![ShaderResource::Output], vec![p]).is_err());
    assert!(
        ShaderKernel::new(
            ShaderSources {
                metal: Some("native".into()),
                hlsl: None
            },
            "main",
            [32, 64, 1],
            vec![]
        )
        .is_err()
    );
    assert!(
        ShaderKernel::new(
            ShaderSources {
                metal: Some("native".into()),
                hlsl: None
            },
            "main",
            [8, 8, 1],
            vec![ShaderBinding {
                name: "output".into(),
                slot: 30,
                kind: ShaderBindingKind::WriteTexture(F)
            }]
        )
        .is_err()
    );
}

#[test]
fn pipeline_snapshot_fingerprints_are_stable_and_do_not_format_image_payloads() {
    let pipeline = ShaderPipeline::new(
        vec![
            ShaderResource::Output,
            ShaderResource::Image {
                size: [256, 256],
                format: ShaderTextureFormat::Rgba8Unorm,
                bytes: vec![255; 256 * 256 * 4].into(),
            },
        ],
        vec![pass(&[(
            3,
            ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
            0,
        )])],
    )
    .unwrap();
    let key = format!("{pipeline:?}");
    assert_eq!(key, format!("{:?}", pipeline.clone()));
    assert!(key.len() < 256);
    assert_ne!(
        key,
        format!(
            "{:?}",
            pipeline
                .with_resource(
                    1,
                    ShaderResource::Image {
                        size: [1, 1],
                        format: ShaderTextureFormat::Rgba8Unorm,
                        bytes: Rc::from([0; 4])
                    }
                )
                .unwrap()
        )
    );
    assert!(pipeline.with_input_radius(f32::NAN).is_err());
    assert!(pipeline.with_input_radius(-1.0).is_err());
    assert_eq!(
        pipeline.with_input_radius(3.0).unwrap().sample_outset(),
        Some(4)
    );
}

#[test]
fn public_invocations_validate_uniforms_external_indices_types_and_aliases() {
    use ShaderBindingKind as B;
    use ShaderResourceKind as K;
    let resources = vec![
        ShaderResource::Uniform(vec![0; 32].into()),
        ShaderResource::External {
            index: 0,
            kind: K::TextureArray,
        },
        ShaderResource::External {
            index: 1,
            kind: K::TextureTable(64),
        },
        ShaderResource::External {
            index: 2,
            kind: K::Texture(ShaderTextureFormat::Rgba8Unorm),
        },
    ];
    let mut p = pass(&[
        (0, B::Uniform(32), 0),
        (12, B::TextureArray, 1),
        (30, B::TextureTable(64), 2),
        (3, B::WriteTexture(ShaderTextureFormat::Rgba8Unorm), 3),
    ]);
    p.dispatch = ShaderDispatch::Groups([1; 3]);
    let pipeline = ShaderPipeline::new(resources.clone(), vec![p.clone()]).unwrap();
    assert!(!pipeline.is_canvas_filter());
    assert_eq!(pipeline.external_count(), 3);
    assert!(
        pipeline
            .with_resource(0, ShaderResource::Uniform(vec![0; 28].into()))
            .is_err()
    );
    assert!(
        pipeline
            .with_resource(
                1,
                ShaderResource::External {
                    index: 0,
                    kind: K::Buffer
                }
            )
            .is_err()
    );
    assert!(
        pipeline
            .with_resource(
                2,
                ShaderResource::External {
                    index: 0,
                    kind: K::TextureTable(64)
                }
            )
            .is_err()
    );
    let mut bad = p;
    bad.bindings[3].1 = 1;
    assert!(ShaderPipeline::new(resources, vec![bad]).is_err());
    assert!(ShaderPipeline::new(vec![ShaderResource::Uniform(vec![0; 3].into())], vec![]).is_err());
}

#[cfg(tileink_native_runtime)]
#[test]
fn precompiled_filter_kernels_use_the_public_constructor_and_full_binding_contract() {
    for artifact in crate::NATIVE_SHADER_ARTIFACTS
        .iter()
        .filter(|a| a.entry.starts_with("filter_") || a.entry.starts_with("progressive_blur_"))
    {
        let kernel = ShaderKernel::precompiled(artifact).unwrap();
        assert_eq!(
            kernel.bindings.len(),
            artifact.bindings.iter().filter(|b| !b.internal).count()
        );
        assert!(kernel.bindings.iter().any(|b| b.kind.writable()));
    }
}

#[test]
fn compact_tile_ids_cannot_be_mutated_through_a_pipeline_binding() {
    let p = pass(&[
        (8, ShaderBindingKind::ReadWriteBuffer, 1),
        (
            3,
            ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
            0,
        ),
    ]);
    assert!(
        ShaderPipeline::new(
            vec![ShaderResource::Output, ShaderResource::ActiveTiles],
            vec![p]
        )
        .is_err()
    );
}

#[test]
fn zero_group_passes_do_not_initialize_intermediates_or_canvas_output() {
    let mut write = pass(&[(2, ShaderBindingKind::WriteTexture(F), 0)]);
    write.dispatch = ShaderDispatch::Groups([0, 1, 1]);
    let finish = pass(&[
        (2, ShaderBindingKind::Texture, 0),
        (
            3,
            ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
            1,
        ),
    ]);
    assert!(
        ShaderPipeline::new(
            vec![
                ShaderResource::Texture {
                    extent: ShaderExtent::FULL,
                    format: F
                },
                ShaderResource::Output
            ],
            vec![write, finish]
        )
        .is_err()
    );
    let mut output = pass(&[(
        3,
        ShaderBindingKind::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
        0,
    )]);
    output.dispatch = ShaderDispatch::Groups([1, 0, 1]);
    assert!(ShaderPipeline::new(vec![ShaderResource::Output], vec![output]).is_err());
}
