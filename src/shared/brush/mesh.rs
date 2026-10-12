//! Immutable, compiled quad meshes. Spatial bins keep fragment work local instead
//! of searching every patch. Geometry is bilinear; colors interpolate in premultiplied
//! sRGB, with optional smoothstep weights (continuous edges, no color overshoot).
use std::{fmt, rc::Rc};

use peniko::Color;

use super::{IDENTITY_TRANSFORM, premul_f32_to_u32};

pub(crate) const MESH_PATCH_WORDS: usize = 16;
pub(crate) const MESH_MAX_BINS: usize = 32;

/// Color interpolation within each mesh quad. Both modes share identical edges.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MeshInterpolation {
    /// Cubic smoothstep weights in each parameter direction; no color overshoot.
    #[default]
    Smooth,
    /// Bilinear weights in the quad's parameter space.
    Bilinear,
}

/// A row-major grid of colored vertices, with movable, straight-sided convex quads.
///
/// Positions use the same local coordinates as the painted geometry. The brush is
/// transparent outside the mesh boundary; clip a larger mesh for a filled shape.
/// Shared vertices make adjacent patch colors continuous. Smooth interpolation
/// does not promise continuous spatial derivatives across a distorted mesh.
#[derive(Clone, Debug)]
pub struct MeshGradient {
    pub(crate) transform: [f32; 6],
    pub(super) bounds: [f32; 4],
    pub(super) bins: [u32; 2],
    pub(super) patch_base: u32,
    pub(super) interpolation: MeshInterpolation,
    pub(super) payload: Rc<[u32]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeshGradientError {
    InvalidDimensions,
    VertexCount,
    NonFinite,
    /// Folded, nearly degenerate, inconsistently wound or self-intersecting mesh.
    InvalidTopology,
    TooLarge,
}

impl fmt::Display for MeshGradientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidDimensions => "mesh needs at least two columns and two rows",
            Self::VertexCount => "mesh position/color counts must equal columns times rows",
            Self::NonFinite => "mesh positions and colors must be finite",
            Self::InvalidTopology => {
                "mesh must have convex, consistently wound quads and a simple boundary"
            }
            Self::TooLarge => "mesh exceeds the GPU word-addressable storage limit",
        })
    }
}
impl std::error::Error for MeshGradientError {}

impl MeshGradient {
    /// Builds and validates a mesh once; cloning and coordinate conversion share
    /// its compiled payload. Both clockwise and counterclockwise winding work.
    ///
    /// # Errors
    /// Returns an error for invalid grid dimensions/counts, nonfinite positions/colors, unsupported grid size, or folded/degenerate topology.
    pub fn new(
        columns: usize,
        rows: usize,
        positions: impl AsRef<[[f32; 2]]>,
        colors: impl AsRef<[Color]>,
    ) -> Result<Self, MeshGradientError> {
        use MeshGradientError::{
            InvalidDimensions, InvalidTopology, NonFinite, TooLarge, VertexCount,
        };
        if columns < 2 || rows < 2 {
            return Err(InvalidDimensions);
        }
        let count = columns.checked_mul(rows).ok_or(TooLarge)?;
        let positions = positions.as_ref();
        let colors = colors.as_ref();
        if positions.len() != count || colors.len() != count {
            return Err(VertexCount);
        }
        if !positions.iter().flatten().all(|v| v.is_finite())
            || !colors.iter().flat_map(|c| c.components).all(f32::is_finite)
        {
            return Err(NonFinite);
        }
        let patches = (columns - 1).checked_mul(rows - 1).ok_or(TooLarge)?;
        if patches > (u32::MAX as usize / 4) / MESH_PATCH_WORDS {
            return Err(TooLarge);
        }
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for p in positions {
            bounds[0] = bounds[0].min(p[0]);
            bounds[1] = bounds[1].min(p[1]);
            bounds[2] = bounds[2].max(p[0]);
            bounds[3] = bounds[3].max(p[1]);
        }
        if !(bounds[2] - bounds[0]).is_finite() || !(bounds[3] - bounds[1]).is_finite() {
            return Err(InvalidTopology);
        }
        let bins = [
            (columns - 1).min(MESH_MAX_BINS),
            (rows - 1).min(MESH_MAX_BINS),
        ];
        let mut lists = vec![Vec::new(); bins[0] * bins[1]];
        let mut records = Vec::with_capacity(patches * MESH_PATCH_WORDS);
        let mut winding = None;
        for y in 0..rows - 1 {
            for x in 0..columns - 1 {
                let indices = [
                    y * columns + x,
                    y * columns + x + 1,
                    (y + 1) * columns + x,
                    (y + 1) * columns + x + 1,
                ];
                let quad = indices.map(|i| positions[i]);
                let bb = [
                    quad.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
                    quad.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
                    quad.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max),
                    quad.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max),
                ];
                let size = [bb[2] - bb[0], bb[3] - bb[1]];
                if size
                    .iter()
                    .any(|v| *v < f32::MIN_POSITIVE || !v.is_finite())
                {
                    return Err(InvalidTopology);
                }
                let q = quad.map(|p| {
                    [
                        (f64::from(p[0]) - f64::from(bb[0])) / f64::from(size[0]),
                        (f64::from(p[1]) - f64::from(bb[1])) / f64::from(size[1]),
                    ]
                });
                let polygon = [q[0], q[1], q[3], q[2]];
                for i in 0..4 {
                    let a = sub(polygon[(i + 1) % 4], polygon[i]);
                    let b = sub(polygon[(i + 2) % 4], polygon[(i + 1) % 4]);
                    let det = cross(a, b);
                    // Bound f32 coefficient-rounding amplification before GPU inversion.
                    if det.abs() <= 1e-4
                        || winding.is_some_and(|sign| sign != det.is_sign_positive())
                    {
                        return Err(InvalidTopology);
                    }
                    winding = Some(det.is_sign_positive());
                }
                records.extend(bb.map(f32::to_bits));
                let b = sub(q[1], q[0]);
                let c = sub(q[2], q[0]);
                let d = sub(sub(q[3], q[2]), b);
                records.extend(
                    [q[0], b, c, d]
                        .into_iter()
                        .flatten()
                        .map(|v| (v as f32).to_bits()),
                );
                records
                    .extend(indices.map(|i| premul_f32_to_u32(colors[i].premultiply().components)));
                let patch = (y * (columns - 1) + x) as u32;
                let range = |axis: usize, upper: bool| {
                    let value = (f64::from(bb[axis + if upper { 2 } else { 0 }])
                        - f64::from(bounds[axis]))
                        / (f64::from(bounds[axis + 2]) - f64::from(bounds[axis]))
                        * bins[axis] as f64;
                    // Include both sides of a bin edge, including f32 rounding drift.
                    (value + if upper { 0.0001 } else { -0.0001 })
                        .floor()
                        .clamp(0.0, (bins[axis] - 1) as f64) as usize
                };
                for by in range(1, false)..=range(1, true) {
                    for bx in range(0, false)..=range(0, true) {
                        lists[by * bins[0] + bx].push(patch);
                    }
                }
            }
        }
        // Consistent nonzero local Jacobians and a simple boundary establish an injective
        // structured mesh. Testing the perimeter avoids quadratic all-patch scans.
        validate_boundary(columns, rows, positions)?;
        let references: usize = lists.iter().map(Vec::len).sum();
        let patch_base = lists
            .len()
            .checked_add(1)
            .and_then(|n| n.checked_add(references))
            .ok_or(TooLarge)?;
        let length = patch_base.checked_add(records.len()).ok_or(TooLarge)?;
        if length > (u32::MAX as usize / 4) - super::ENCODED_BRUSH_HEADER_WORDS {
            return Err(TooLarge);
        }
        let mut payload = Vec::with_capacity(length);
        let mut cursor = lists.len() + 1;
        for list in &lists {
            payload.push(cursor as u32);
            cursor += list.len();
        }
        payload.push(cursor as u32);
        payload.extend(lists.into_iter().flatten());
        payload.extend(records);
        Ok(Self {
            transform: IDENTITY_TRANSFORM,
            bounds,
            bins: bins.map(|v| v as u32),
            patch_base: patch_base as u32,
            interpolation: MeshInterpolation::Smooth,
            payload: payload.into(),
        })
    }

    #[must_use]
    pub const fn with_interpolation(mut self, interpolation: MeshInterpolation) -> Self {
        self.interpolation = interpolation;
        self
    }
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn validate_boundary(
    columns: usize,
    rows: usize,
    positions: &[[f32; 2]],
) -> Result<(), MeshGradientError> {
    let boundary: Vec<_> = (0..columns)
        .chain((1..rows).map(|y| y * columns + columns - 1))
        .chain((0..columns - 1).rev().map(|x| (rows - 1) * columns + x))
        .chain((1..rows - 1).rev().map(|y| y * columns))
        .map(|i| positions[i].map(f64::from))
        .collect();
    let n = boundary.len();
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let (a, b, c, d) = (
                boundary[i],
                boundary[(i + 1) % n],
                boundary[j],
                boundary[(j + 1) % n],
            );
            let ab = sub(b, a);
            let cd = sub(d, c);
            let o = [
                cross(ab, sub(c, a)),
                cross(ab, sub(d, a)),
                cross(cd, sub(a, c)),
                cross(cd, sub(b, c)),
            ];
            let overlaps = (0..2).all(|axis| {
                a[axis].min(b[axis]) <= c[axis].max(d[axis])
                    && c[axis].min(d[axis]) <= a[axis].max(b[axis])
            });
            if overlaps && o[0] * o[1] <= 0.0 && o[2] * o[3] <= 0.0 {
                return Err(MeshGradientError::InvalidTopology);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
