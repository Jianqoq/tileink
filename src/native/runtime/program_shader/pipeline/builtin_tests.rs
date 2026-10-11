use super::*;
use crate::native::runtime::program::filter::pipeline::record as record_builtin;
use crate::native::shaders::{BindingKind, NativeShaderArtifact};

fn bindings(
    batch: &mut ComputeBatch,
    artifact: &NativeShaderArtifact,
) -> Result<Vec<(u32, ResourceId)>> {
    artifact
        .bindings
        .iter()
        .filter(|b| !b.internal)
        .map(|b| {
            let id = match b.kind {
                BindingKind::Uniform | BindingKind::Read | BindingKind::Write => {
                    batch.buffer(vec![0; b.size.max(4) as usize])?
                }
                BindingKind::Texture
                | BindingKind::TextureWrite
                | BindingKind::TextureReadWrite => {
                    assert_eq!(b.size, 4, "current built-in filter surfaces are RGBA8");
                    batch.texture_rgba8([3, 2], vec![0; 24])?
                }
                BindingKind::TextureArray => batch.texture_array_rgba8([3, 2, 2], vec![0; 48])?,
                BindingKind::TextureTable => {
                    let image = batch.texture_rgba8([3, 2], vec![0; 24])?;
                    batch.texture_table(&vec![image; b.count as usize])?
                }
                BindingKind::Sampler => batch.sampler(SamplerFilter::Linear)?,
            };
            Ok((b.slot, id))
        })
        .collect()
}

#[test]
fn every_builtin_filter_pipeline_preserves_native_layout_and_resident_resources() -> Result<()> {
    let mut count = 0;
    for artifact in crate::NATIVE_SHADER_ARTIFACTS
        .iter()
        .filter(|a| a.entry.starts_with("filter_") || a.entry.starts_with("progressive_blur_"))
    {
        let mut batch = ComputeBatch::new();
        let mut bindings = bindings(&mut batch, artifact)?;
        bindings.reverse(); // Invocation order is independent from native slot order.
        let resource_count = batch.resources().len();
        record_builtin(&mut batch, artifact.entry, &bindings, [2, 3, 1])?;
        assert_eq!(
            batch.resources().len(),
            resource_count,
            "{} made an extra upload/copy allocation",
            artifact.entry
        );
        let pass = &batch.passes()[0];
        assert_eq!(pass.entry(), artifact.entry);
        assert_eq!(pass.shader.cache_key, artifact.cache_key);
        assert_eq!(pass.shader.workgroup, artifact.workgroup);
        assert!(
            pass.native_source.is_none(),
            "built-ins must retain precompiled kernels"
        );
        assert_eq!(pass.grid, [2, 3, 1]);
        for (binding, id) in &pass.bindings {
            assert_eq!(
                Some(id),
                bindings
                    .iter()
                    .find(|(slot, _)| *slot == binding.slot)
                    .map(|(_, id)| id)
            );
        }
        count += 1;
    }
    assert!(count >= 30, "the complete filter catalog must be exercised");
    Ok(())
}

#[test]
fn builtin_pipelines_reject_invalid_bindings_and_preserve_read_only_aliases() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let c = batch.buffer(vec![
        0;
        std::mem::size_of::<
            crate::shared::filter_config::FilterConfig,
        >()
    ])?;
    let tiles = batch.buffer(vec![0; 4])?;
    let input = batch.texture_rgba8([3, 2], vec![0; 24])?;
    let output = batch.texture_rgba8([3, 2], vec![0; 24])?;
    let valid = [(0, c), (1, input), (2, input), (3, output), (8, tiles)];
    for grid in [[65536, 1, 1], [65535, 65535, 2]] {
        assert!(record_builtin(&mut batch, "filter_blend_region", &valid, grid).is_err());
    }
    for invalid in [
        vec![(0, c), (1, input), (2, input), (3, input), (8, tiles)],
        vec![(0, c), (1, input), (1, input), (3, output), (8, tiles)],
        vec![(0, tiles), (1, input), (2, input), (3, output), (8, tiles)],
        vec![(0, c), (1, c), (2, input), (3, output), (8, tiles)],
        valid[..4].to_vec(),
    ] {
        assert!(record_builtin(&mut batch, "filter_blend_region", &invalid, [1; 3]).is_err());
    }
    assert!(record_builtin(&mut batch, "missing_kernel", &valid, [1; 3]).is_err());
    let mut other = ComputeBatch::new();
    let foreign = other.buffer(vec![0; 4])?;
    let mut invalid = valid;
    invalid[4].1 = foreign;
    assert!(record_builtin(&mut batch, "filter_blend_region", &invalid, [1; 3]).is_err());
    assert!(batch.commands().is_empty());
    record_builtin(&mut batch, "filter_blend_region", &valid, [0, 1, 1])?;
    assert!(batch.commands().is_empty());
    record_builtin(&mut batch, "filter_blend_region", &valid, [1; 3])?;
    assert_eq!(batch.passes().len(), 1);
    Ok(())
}

#[test]
fn builtin_pipeline_tables_validate_counts_and_destination_aliases() -> Result<()> {
    let artifact = crate::NATIVE_SHADER_ARTIFACTS
        .iter()
        .find(|a| a.entry == "progressive_blur_resolve")
        .unwrap();
    let mut batch = ComputeBatch::new();
    let mut bound = bindings(&mut batch, artifact)?;
    let count = artifact
        .bindings
        .iter()
        .find(|b| b.kind == BindingKind::TextureTable)
        .unwrap()
        .count;
    let target = bound.iter().find(|(slot, _)| *slot == 3).unwrap().1;
    let table_index = bound.iter().position(|(slot, _)| *slot == 30).unwrap();
    let valid_table = bound[table_index].1;
    for images in [
        vec![target; count as usize],
        vec![target; count as usize - 1],
    ] {
        bound[table_index].1 = batch.texture_table(&images)?;
        assert!(record_builtin(&mut batch, artifact.entry, &bound, [1; 3]).is_err());
    }
    assert!(batch.passes().is_empty());
    bound[table_index].1 = valid_table;
    record_builtin(&mut batch, artifact.entry, &bound, [1; 3])?;
    Ok(())
}

#[test]
fn builtin_pipeline_plan_reuse_does_not_capture_handles_or_stale_group_counts() -> Result<()> {
    for grid in [[1, 2, 1], [1, 2, 1], [3, 1, 1], [1, 2, 1]] {
        let mut batch = ComputeBatch::new();
        let uniform = batch.buffer(vec![
            0;
            std::mem::size_of::<
                crate::shared::filter_config::FilterConfig,
            >()
        ])?;
        let tiles = batch.buffer(vec![0; 4])?;
        let target = batch.texture_rgba8([3, 2], vec![0; 24])?;
        record_builtin(
            &mut batch,
            "filter_clear_region",
            &[(8, tiles), (3, target), (0, uniform)],
            grid,
        )?;
        let pass = &batch.passes()[0];
        assert_eq!(pass.grid, grid);
        assert_eq!(
            pass.bindings.iter().find(|(b, _)| b.slot == 3).unwrap().1,
            target
        );
        assert_eq!(
            pass.bindings.iter().find(|(b, _)| b.slot == 0).unwrap().1,
            uniform
        );
    }
    Ok(())
}
