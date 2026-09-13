use super::*;

#[test]
fn atlas_growth_preserves_clean_existing_pages() {
    check_atlas_growth_resources(false, false, false);
}

#[test]
fn atlas_growth_does_not_reupload_clean_standalone_textures() {
    check_atlas_growth_resources(true, false, true);
}

#[test]
fn explicit_full_upload_refreshes_standalone_textures_after_atlas_growth() {
    check_atlas_growth_resources(true, true, true);
}

#[test]
fn atlas_growth_preserves_clean_standalone_texture_pixels() {
    check_atlas_growth_resources(true, false, false);
}

fn check_atlas_growth_resources(standalone: bool, force_all: bool, write_canary: bool) {
    if !run_wgpu_tests() {
        return;
    }
    let mut renderer = new_test_renderer(2, 1, Color::TRANSPARENT);
    let mut images = crate::shared::image_resource::ImageResourceStore::default();
    let empty = crate::shared::image_resource::ImageResourceStore::default();
    // Real packing rules require four padded 1022-square images to fill a
    // 2048 page. A 2048-wide image instead uses the independent texture table.
    for key in 1..=4 {
        images.insert(
            ImageKey::new(key),
            Image::from_rgba8(
                1022,
                1022,
                [key as u8 * 20, 40, 60, 255].repeat(1022 * 1022),
            ),
        );
    }
    if standalone {
        images.insert(
            ImageKey::new(100),
            Image::from_rgba8(2048, 32, [160, 40, 60, 255].repeat(2048 * 32)),
        );
    }
    let first = images.upload_merged(&empty, 2048, 2, u32::from(standalone), None);
    assert_eq!(first.atlas_page_size(), 2048);
    assert_eq!(first.atlas_page_count(), 1);
    renderer
        .scene_buffers
        .upload_image_resources(&renderer.device, &renderer.queue, &first, false);
    renderer.queue.submit([]);
    if write_canary {
        assert!(standalone);
        assert_eq!(first.textures().len(), 1);
        // A GPU canary exposes an unnecessary write to an allocation outside the
        // dirty set. The test checks the uploader's write footprint, not new
        // renderer semantics for application-managed image contents.
        let bindings = renderer.scene_buffers.image_resource_bindings();
        let texture = bindings.texture_views[first.textures()[0].index as usize].texture();
        renderer.queue.write_texture(
            ::wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: ::wgpu::Origin3d { x: 1, y: 1, z: 0 },
                aspect: ::wgpu::TextureAspect::All,
            },
            &[7, 11, 13, 255],
            ::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            ::wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        renderer.queue.submit([]);
    }

    images.insert(
        ImageKey::new(5),
        Image::from_rgba8(1022, 1022, [100, 40, 60, 255].repeat(1022 * 1022)),
    );
    let second = images.upload_merged(&empty, 2048, 2, u32::from(standalone), Some(&first));
    assert_eq!(second.atlas_page_count(), 2);
    assert!(!second.atlas_pages()[0].dirty);
    assert!(second.atlas_pages()[1].dirty);
    if standalone {
        assert_eq!(second.textures().len(), 1);
        assert!(!second.textures()[0].dirty);
    }
    renderer.scene_buffers.upload_image_resources(
        &renderer.device,
        &renderer.queue,
        &second,
        force_all,
    );

    // The atlas is sampled-only. Probe its real binding rather than adding a
    // readback usage to production textures just for this regression test.
    let shader = renderer
        .device
        .create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink atlas growth probe"),
            source: ::wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
                r#"
            @group(0) @binding(0) var atlas: texture_2d_array<f32>;
            @group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
            @group(0) @binding(2) var independent: texture_2d<f32>;
            @compute @workgroup_size(1)
            fn main(@builtin(global_invocation_id) id: vec3<u32>) {
                var color: vec4<f32>;
                if id.x == 2u {
                    color = textureLoad(independent, vec2<i32>(1, 1), 0);
                } else {
                    color = textureLoad(atlas, vec2<i32>(1, 1), i32(id.x), 0);
                }
                textureStore(output, vec2<i32>(i32(id.x), 0), color);
            }
        "#,
            )),
        });
    let pipeline = renderer
        .device
        .create_compute_pipeline(&::wgpu::ComputePipelineDescriptor {
            label: Some("tileink atlas growth probe"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let output_width = if standalone { 3 } else { 2 };
    let output = renderer.device.create_texture(&::wgpu::TextureDescriptor {
        label: Some("tileink atlas growth probe output"),
        size: ::wgpu::Extent3d {
            width: output_width,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: ::wgpu::TextureDimension::D2,
        format: ::wgpu::TextureFormat::Rgba8Unorm,
        usage: ::wgpu::TextureUsages::STORAGE_BINDING | ::wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = output.create_view(&Default::default());
    let bindings = renderer.scene_buffers.image_resource_bindings();
    let independent = if standalone {
        &bindings.texture_views[second.textures()[0].index as usize]
    } else {
        bindings.dummy_texture
    };
    let group = renderer
        .device
        .create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink atlas growth probe"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                ::wgpu::BindGroupEntry {
                    binding: 0,
                    resource: ::wgpu::BindingResource::TextureView(bindings.atlas),
                },
                ::wgpu::BindGroupEntry {
                    binding: 1,
                    resource: ::wgpu::BindingResource::TextureView(&view),
                },
                ::wgpu::BindGroupEntry {
                    binding: 2,
                    resource: ::wgpu::BindingResource::TextureView(independent),
                },
            ],
        });
    let mut encoder = renderer.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(output_width, 1, 1);
    }
    renderer.queue.submit([encoder.finish()]);
    let mut expected = vec![20, 40, 60, 255, 100, 40, 60, 255];
    if standalone {
        expected.extend_from_slice(if force_all || !write_canary {
            &[160, 40, 60, 255]
        } else {
            &[7, 11, 13, 255]
        });
    }
    assert_eq!(
        read_texture_rgba8(&renderer.device, &renderer.queue, &output, output_width, 1),
        expected
    );
}
