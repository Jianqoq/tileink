use super::*;
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .enumerate()
            .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || i > 0 && c.is_ascii_digit())
}
fn extent(e: ShaderExtent) -> bool {
    match e {
        ShaderExtent::Surface { divisor } => !divisor.contains(&0),
        ShaderExtent::Fixed(size) => {
            !size.contains(&0) && size.iter().all(|&n| n <= i32::MAX as u32)
        }
    }
}
pub(super) fn kernel(
    s: &ShaderSources,
    entry: &str,
    workgroup: [u32; 3],
    bindings: &[ShaderBinding],
) -> Result<(), ShaderError> {
    // Share the single-kernel source/entry checks without adding a second compiler.
    if [s.metal.as_deref(), s.hlsl.as_deref()]
        .into_iter()
        .flatten()
        .any(|s| s.trim().is_empty())
        || s.metal.is_none() && s.hlsl.is_none()
    {
        return Err(ShaderError::MissingSource);
    }
    if !identifier(entry) {
        return Err(ShaderError::InvalidEntry);
    }
    interface(workgroup, bindings)
}
pub(super) fn interface(
    workgroup: [u32; 3],
    bindings: &[ShaderBinding],
) -> Result<(), ShaderError> {
    if workgroup.contains(&0)
        || workgroup
            .iter()
            .try_fold(1u32, |a, &b| a.checked_mul(b))
            .is_none_or(|n| n > 1024)
    {
        return Err(ShaderError::InvalidDispatch);
    }
    for (i, b) in bindings.iter().enumerate() {
        if b.slot > 30
            || (b.slot == 30 && !matches!(b.kind, ShaderBindingKind::TextureTable(_)))
            || b.kind == ShaderBindingKind::Sampler && b.slot > 15
            || (b.kind == ShaderBindingKind::Config && b.slot != 0)
            || matches!(b.kind, ShaderBindingKind::Uniform(n) if n == 0 || n > 65536 || !n.is_multiple_of(4))
            || matches!(b.kind, ShaderBindingKind::TextureTable(0))
            || !identifier(&b.name)
            || bindings[..i]
                .iter()
                .any(|a| a.slot == b.slot || a.name == b.name)
        {
            return Err(ShaderError::InvalidBinding);
        }
    }
    if !bindings.iter().any(|b| b.kind.writable()) {
        return Err(ShaderError::InvalidBinding);
    }
    Ok(())
}
pub(super) fn pipeline(
    resources: &[ShaderResource],
    passes: &[ShaderPass],
) -> Result<(), ShaderError> {
    let outputs = resources
        .iter()
        .filter(|r| matches!(r, ShaderResource::Output))
        .count();
    let externals: Vec<_> = resources
        .iter()
        .filter_map(|r| {
            if let ShaderResource::External { index, .. } = r {
                Some(*index)
            } else {
                None
            }
        })
        .collect();
    if (0..externals.len()).any(|i| externals.iter().filter(|&&n| n == i).count() != 1) {
        return Err(ShaderError::InvalidResource);
    }
    if passes.is_empty() || outputs > 1 || (outputs == 0 && externals.is_empty()) {
        return Err(ShaderError::InvalidResource);
    }
    let inputs: Vec<_> = resources
        .iter()
        .filter_map(|r| {
            if let ShaderResource::Input(i) = r {
                Some(*i)
            } else {
                None
            }
        })
        .collect();
    if (0..inputs.len()).any(|i| inputs.iter().filter(|&&n| n == i).count() != 1) {
        return Err(ShaderError::InvalidResource);
    }
    for r in resources {
        let valid = match r {
            ShaderResource::Texture { extent: e, .. } => extent(*e),
            ShaderResource::Config(c) => {
                extent(c.extent)
                    && c.sample_radius.is_finite()
                    && c.sample_radius >= 0.0
                    && c.uniform_words <= u32::MAX / 4
            }
            ShaderResource::Uniform(bytes) => {
                !bytes.is_empty()
                    && bytes.len().is_multiple_of(4)
                    && bytes.len() <= u32::MAX as usize
            }
            ShaderResource::External {
                kind: ShaderResourceKind::TextureTable(count),
                ..
            } => *count > 0,
            ShaderResource::Image {
                size,
                format,
                bytes,
            } => {
                extent(ShaderExtent::Fixed(*size))
                    && (size[0] as usize)
                        .checked_mul(size[1] as usize)
                        .and_then(|n| n.checked_mul(format.bytes_per_pixel()))
                        == Some(bytes.len())
            }
            ShaderResource::Buffer(words) => {
                !words.is_empty() && words.len() <= u32::MAX as usize / 4
            }
            _ => true,
        };
        if !valid {
            return Err(ShaderError::InvalidResource);
        }
    }
    let mut initialized: Vec<bool> = resources
        .iter()
        .map(|r| !matches!(r, ShaderResource::Texture { .. } | ShaderResource::Output))
        .collect();
    for p in passes {
        if p.bindings.len() != p.kernel.bindings.len() {
            return Err(ShaderError::InvalidBinding);
        }
        match p.dispatch {
            ShaderDispatch::Pixels if p.kernel.workgroup != [256, 1, 1] => {
                return Err(ShaderError::InvalidDispatch);
            }
            ShaderDispatch::Image(e) | ShaderDispatch::Region { extent: e, .. }
                if !extent(e) || p.kernel.workgroup[2] != 1 =>
            {
                return Err(ShaderError::InvalidDispatch);
            }
            ShaderDispatch::Groups(g)
                if g.iter().any(|&n| n > 65535)
                    || g.iter().map(|&n| u64::from(n)).product::<u64>() > u64::from(u32::MAX) =>
            {
                return Err(ShaderError::InvalidDispatch);
            }
            _ => {}
        }
        let mut used = Vec::new();
        for b in p.kernel.bindings.iter() {
            let mut found = p.bindings.iter().filter(|(slot, _)| *slot == b.slot);
            let &(_, index) = found.next().ok_or(ShaderError::InvalidBinding)?;
            let r = resources.get(index).ok_or(ShaderError::InvalidResource)?;
            if found.next().is_some() {
                return Err(ShaderError::InvalidBinding);
            }
            let valid = match (b.kind, r) {
                (ShaderBindingKind::Config, ShaderResource::Config(_)) => true,
                (ShaderBindingKind::Uniform(n), ShaderResource::Uniform(bytes)) => {
                    bytes.len() == n as usize
                }
                (
                    ShaderBindingKind::Uniform(_),
                    ShaderResource::External {
                        kind: ShaderResourceKind::Buffer,
                        ..
                    },
                ) => true,
                (ShaderBindingKind::ReadWriteBuffer, ShaderResource::ActiveTiles) => false,
                (ShaderBindingKind::ReadBuffer | ShaderBindingKind::ReadWriteBuffer, r) => {
                    r.kind() == ShaderResourceKind::Buffer
                        && !matches!(r, ShaderResource::Config(_) | ShaderResource::Uniform(_))
                }
                (ShaderBindingKind::Sampler, r) => r.kind() == ShaderResourceKind::Sampler,
                (ShaderBindingKind::TextureArray, r) => {
                    r.kind() == ShaderResourceKind::TextureArray
                }
                (ShaderBindingKind::TextureTable(n), r) => {
                    r.kind() == ShaderResourceKind::TextureTable(n)
                }
                (ShaderBindingKind::Texture, r) => {
                    matches!(r.kind(), ShaderResourceKind::Texture(_)) && initialized[index]
                }
                (ShaderBindingKind::WriteTexture(f), r) => {
                    r.kind() == ShaderResourceKind::Texture(f)
                        && !matches!(r, ShaderResource::Input(_) | ShaderResource::Image { .. })
                }
                (ShaderBindingKind::ReadWriteTexture(f), r) => {
                    r.kind() == ShaderResourceKind::Texture(f)
                        && initialized[index]
                        && !matches!(r, ShaderResource::Input(_) | ShaderResource::Image { .. })
                }
                _ => false,
            };
            if !valid
                || used
                    .iter()
                    .any(|&(old, writable)| old == index && (writable || b.kind.writable()))
            {
                return Err(ShaderError::InvalidBinding);
            }
            used.push((index, b.kind.writable()));
        }
        // Reads in a pass cannot observe its own writes. Availability advances
        // only at pass boundaries where native backends insert memory dependencies.
        for (index, writable) in used {
            // Empty explicit launches validate bindings but perform no writes.
            if writable && !matches!(p.dispatch, ShaderDispatch::Groups(g) if g.contains(&0)) {
                initialized[index] = true;
            }
        }
    }
    if resources
        .iter()
        .enumerate()
        .any(|(i, r)| matches!(r, ShaderResource::Output) && !initialized[i])
    {
        return Err(ShaderError::InvalidResource);
    }
    Ok(())
}
