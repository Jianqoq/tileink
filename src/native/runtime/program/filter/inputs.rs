use super::{ComputeBatch, FilterConfig, ResourceId, Result, region};

#[derive(Clone, Copy, Debug)]
pub enum InputFilter {
    Blend {
        source: ResourceId,
        backdrop: ResourceId,
    },
    Composite {
        source: ResourceId,
        backdrop: ResourceId,
    },
    Mask {
        mask: ResourceId,
    },
}

/// Explicit read resources avoid dummy textures; two read-only inputs may alias.
pub fn encode(
    batch: &mut ComputeBatch,
    kernel: InputFilter,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    target: ResourceId,
) -> Result<()> {
    let (entry, reads) = match kernel {
        InputFilter::Blend { source, backdrop } => {
            ("filter_blend_region", vec![(1, source), (2, backdrop)])
        }
        InputFilter::Composite { source, backdrop } => {
            if config.matrix_bias.iter().any(|v| !v.is_finite()) {
                return Err("nonfinite arithmetic composite coefficient".into());
            }
            (
                "filter_composite_inputs_region",
                vec![(1, source), (2, backdrop)],
            )
        }
        InputFilter::Mask { mask } => ("filter_apply_region_mask", vec![(2, mask)]),
    };
    region::record(batch, entry, config, tiles, &reads, target)
}
