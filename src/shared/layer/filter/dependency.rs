//! Input-read footprints are independent of output geometry. This fixes missing
//! Erode/convolution/lighting halos and preserves Wrap's original address domain.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FilterDependency {
    Local(i32),
    WholeRegion,
}

impl FilterDependency {
    pub(crate) fn local_radius(self) -> Option<i32> {
        match self {
            Self::Local(radius) => Some(radius),
            Self::WholeRegion => None,
        }
    }

    pub(crate) fn source_bounds(self, output: Bounds, domain: Bounds) -> Bounds {
        if output.is_empty() {
            return output;
        }
        match self {
            Self::Local(radius) => output.outset(radius).intersect(domain),
            Self::WholeRegion => domain,
        }
    }

    pub(crate) fn affected_output(self, changed: Bounds, input: Bounds, output: Bounds) -> Bounds {
        let changed = changed.intersect(input);
        if changed.is_empty() {
            return changed;
        }
        match self {
            Self::Local(radius) => changed.outset(radius).intersect(output),
            Self::WholeRegion => output,
        }
    }

    /// BoundsInfluence stores a finite conservative spatial summary. Resolve a
    /// whole-domain read here, while execution keeps the explicit domain kind.
    pub(crate) fn damage_outset(self, domain: Bounds) -> i32 {
        match self {
            Self::Local(radius) => radius,
            Self::WholeRegion => domain
                .width()
                .max(domain.height())
                .saturating_sub(1)
                .min(i32::MAX as u32) as i32,
        }
    }

    fn followed_by(self, next: Self) -> Self {
        match (self, next) {
            (Self::Local(a), Self::Local(b)) => Self::Local(a.saturating_add(b)),
            _ => Self::WholeRegion,
        }
    }
}

pub(crate) fn filter_input_bounds(filter: &Filter, region: &Region) -> Bounds {
    match filter_dependency(filter) {
        FilterDependency::Local(radius) => region_bounds(region).outset(radius),
        FilterDependency::WholeRegion => unclipped_filtered_region_bounds(filter, region),
    }
}

pub(crate) fn filter_dependency(filter: &Filter) -> FilterDependency {
    match filter {
        Filter::Chain { filters, .. } => filters
            .iter()
            .map(filter_dependency)
            .fold(FilterDependency::Local(0), FilterDependency::followed_by),
        Filter::Graph { primitives, .. } => primitives
            .iter()
            .map(primitive_dependency)
            .fold(FilterDependency::Local(0), FilterDependency::followed_by),
        Filter::Morphology {
            radius_x, radius_y, ..
        } => FilterDependency::Local(radius_x.max(*radius_y).max(0.0).ceil() as i32),
        Filter::ConvolveMatrix(matrix) => convolution_dependency(matrix),
        Filter::DiffuseLighting(_) | Filter::SpecularLighting(_) => FilterDependency::Local(1),
        _ => FilterDependency::Local(filter_outset(filter)),
    }
}

fn convolution_dependency(matrix: &ConvolveMatrix) -> FilterDependency {
    if matrix.columns == 0 || matrix.rows == 0 || matrix.divisor == 0.0 {
        return FilterDependency::Local(0);
    }
    // The shader uses the anchor without clamping. abs_diff covers anchors
    // outside the kernel as well as asymmetric, even-sized kernels.
    let radius = matrix
        .target_x
        .max((matrix.columns - 1).abs_diff(matrix.target_x))
        .max(matrix.target_y)
        .max((matrix.rows - 1).abs_diff(matrix.target_y));
    if radius > 0 && matrix.edge_mode == ConvolveEdgeMode::Wrap {
        FilterDependency::WholeRegion
    } else {
        FilterDependency::Local(radius.min(i32::MAX as u32) as i32)
    }
}

fn primitive_dependency(primitive: &FilterPrimitive) -> FilterDependency {
    match &primitive.kind {
        FilterPrimitiveKind::Filter(filter) => filter_dependency(filter),
        FilterPrimitiveKind::DisplacementMap(map) => {
            FilterDependency::Local((map.scale_x.abs().max(map.scale_y.abs()) * 0.5).ceil() as i32)
        }
        FilterPrimitiveKind::Tile { source_region } => {
            FilterDependency::Local(bounds_distance(primitive.region, *source_region))
        }
        _ => FilterDependency::Local(0),
    }
}

fn bounds_distance(a: Bounds, b: Bounds) -> i32 {
    (a.x0 - b.x0)
        .abs()
        .max((a.y0 - b.y0).abs())
        .max((a.x1 - b.x1).abs())
        .max((a.y1 - b.y1).abs())
}
