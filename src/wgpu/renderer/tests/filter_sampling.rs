use super::*;

#[test]
fn filter_sampling_ignores_texture_capacity() {
    // The actual production filter helpers must not depend on pooled capacity.
    let logical = (192, 128);
    let pixels: Vec<u8> = (0..logical.1)
        .flat_map(|y| {
            (0..logical.0).flat_map(move |x| {
                [
                    ((x * 73 + y * 19) % 256) as u8,
                    ((x * 29 + y * 67) % 256) as u8,
                    ((x * 11 + y * 47) % 256) as u8,
                    255,
                ]
            })
        })
        .collect();
    run_sampling_cases(logical, &pixels, "compare_sampling", |capacity, result| {
        let differences = result
            .chunks_exact(4)
            .filter(|pixel| *pixel != [0, 0, 0, 255])
            .count();
        assert_eq!(
            differences, 0,
            "filter samples must be independent of allocation capacity {capacity:?}"
        );
    });
}

#[test]
fn filter_sampling_preserves_bilinear_premultiplied_color_and_logical_edges() {
    let pixels = [0, 0, 0, 0, 64, 0, 0, 64, 0, 128, 0, 128, 64, 64, 192, 192];
    // Independent channel-space expectations for corners, center, quarter weights,
    // then clamping beyond left/right/top/bottom. No unpremultiplication is allowed.
    let expected = [
        [0, 0, 0, 0],
        [64, 0, 0, 64],
        [0, 128, 0, 128],
        [64, 64, 192, 192],
        [32, 48, 48, 96],
        [16, 84, 36, 112],
        [0, 64, 0, 64],
        [64, 32, 96, 128],
        [32, 0, 0, 32],
        [32, 96, 96, 160],
    ];
    run_sampling_cases((2, 2), &pixels, "sampling_contract", |capacity, result| {
        for (index, pixel) in result.chunks_exact(4).enumerate() {
            assert_eq!(
                pixel,
                expected[index % expected.len()],
                "bilinear case {index} with capacity {capacity:?}"
            );
        }
    });
}

#[test]
fn filter_sampling_clamps_single_texel_axes() {
    let constant = [16, 32, 48, 64];
    run_sampling_cases(
        (1, 1),
        &constant,
        "sampling_contract",
        |capacity, result| {
            for pixel in result.chunks_exact(4) {
                assert_eq!(pixel, constant, "single texel with capacity {capacity:?}");
            }
        },
    );
    let horizontal = [0, 0, 0, 0, 64, 0, 0, 64];
    let expected_horizontal = [0, 64, 0, 64, 32, 16, 0, 64, 32, 32];
    run_sampling_cases(
        (2, 1),
        &horizontal,
        "sampling_contract",
        |capacity, result| {
            for (index, pixel) in result.chunks_exact(4).enumerate() {
                let value = expected_horizontal[index % 10];
                assert_eq!(
                    pixel,
                    [value, 0, 0, value],
                    "single row with capacity {capacity:?}"
                );
            }
        },
    );
    let vertical = [0, 0, 0, 0, 0, 128, 0, 128];
    let expected_vertical = [0, 0, 128, 128, 64, 96, 64, 64, 0, 128];
    run_sampling_cases(
        (1, 2),
        &vertical,
        "sampling_contract",
        |capacity, result| {
            for (index, pixel) in result.chunks_exact(4).enumerate() {
                let value = expected_vertical[index % 10];
                assert_eq!(
                    pixel,
                    [0, value, 0, value],
                    "single column with capacity {capacity:?}"
                );
            }
        },
    );
}

fn run_sampling_cases(
    logical: (u32, u32),
    pixels: &[u8],
    entry: &str,
    check: impl Fn((u32, u32), &[u8]),
) {
    if !run_wgpu_tests() {
        return;
    }
    let Some((device, queue)) = shared_wgpu_test_device(true) else {
        return;
    };
    let source = external_target(device, logical, "filter sampling exact capacity");
    let output = external_target(device, (512, 256), "filter sampling differences");
    let upload = |texture: &::wgpu::Texture| {
        queue.write_texture(
            texture.as_image_copy(),
            pixels,
            ::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(logical.0 * 4),
                rows_per_image: None,
            },
            ::wgpu::Extent3d {
                width: logical.0,
                height: logical.1,
                depth_or_array_layers: 1,
            },
        )
    };
    upload(&source);
    let shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
        label: Some("filter sampling capacity regression"),
        source: ::wgpu::ShaderSource::Wgsl(
            [
                &include_str!("filter_sampling.wgsl").replace(
                    "TestConfig(192u, 128u)",
                    &format!("TestConfig({}u, {}u)", logical.0, logical.1),
                ),
                include_str!("../../shaders/shared/pixel.wgsl"),
                include_str!("../../shaders/filter/textures.wgsl"),
            ]
            .join("\n")
            .into(),
        ),
    });
    let entries = [
        (
            0,
            ::wgpu::BindingType::Texture {
                sample_type: ::wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: ::wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
        ),
        (
            1,
            ::wgpu::BindingType::Texture {
                sample_type: ::wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: ::wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
        ),
        (
            2,
            ::wgpu::BindingType::StorageTexture {
                access: ::wgpu::StorageTextureAccess::WriteOnly,
                format: ::wgpu::TextureFormat::Rgba8Unorm,
                view_dimension: ::wgpu::TextureViewDimension::D2,
            },
        ),
    ]
    .map(|(binding, ty)| ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty,
        count: None,
    });
    let layout = device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
        label: Some("filter sampling regression"),
        entries: &entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&::wgpu::ComputePipelineDescriptor {
        label: Some("filter sampling regression"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    });
    let source_view = source.create_view(&Default::default());
    let output_view = output.create_view(&Default::default());
    let sentinel = vec![255; 512 * 256 * 4];
    for capacity in [
        logical,
        (logical.0, logical.1 + 16),
        (logical.0 + 1, logical.1 + 1),
        (logical.0 * 2, logical.1 * 2),
        (logical.0 * 2 - 1, logical.1 * 2 - 1),
    ] {
        // Reinitialize every output: an omitted or partial dispatch cannot inherit a pass.
        queue.write_texture(
            output.as_image_copy(),
            &sentinel,
            ::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(512 * 4),
                rows_per_image: None,
            },
            ::wgpu::Extent3d {
                width: 512,
                height: 256,
                depth_or_array_layers: 1,
            },
        );
        let auxiliary = external_target(device, capacity, "filter sampling pooled capacity");
        upload(&auxiliary);
        let auxiliary_view = auxiliary.create_view(&Default::default());
        let entries = [
            (0, ::wgpu::BindingResource::TextureView(&source_view)),
            (1, ::wgpu::BindingResource::TextureView(&auxiliary_view)),
            (2, ::wgpu::BindingResource::TextureView(&output_view)),
        ]
        .map(|(binding, resource)| ::wgpu::BindGroupEntry { binding, resource });
        let bindings = device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &entries,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(64, 32, 1);
        }
        queue.submit([encoder.finish()]);
        let result = read_texture_rgba8(device, queue, &output, 512, 256);
        check(capacity, &result);
    }
}
