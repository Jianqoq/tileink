use super::*;
use crate::shared::brush::{
    Brush, decode_encoded_brush, encoded_brush_word_len, push_encoded_brush,
};

const POSITIONS: [[f32; 2]; 4] = [[0.0, 0.0], [100.0, 0.0], [0.0, 100.0], [100.0, 100.0]];
fn colors() -> [Color; 4] {
    [
        Color::from_rgb8(255, 0, 0),
        Color::from_rgba8(0, 255, 0, 128),
        Color::TRANSPARENT,
        Color::from_rgb8(0, 0, 255),
    ]
}

#[test]
fn mesh_rejects_invalid_dimensions_counts_and_nonfinite_inputs() {
    for (w, h, expected) in [
        (1, 2, MeshGradientError::InvalidDimensions),
        (2, 3, MeshGradientError::VertexCount),
        (usize::MAX, 2, MeshGradientError::TooLarge),
    ] {
        assert_eq!(
            MeshGradient::new(w, h, POSITIONS, colors()).unwrap_err(),
            expected
        );
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut positions = POSITIONS;
        positions[0][0] = value;
        assert_eq!(
            MeshGradient::new(2, 2, positions, colors()).unwrap_err(),
            MeshGradientError::NonFinite
        );
        let mut invalid_colors = colors();
        invalid_colors[0] = Color::new([value, 0.0, 0.0, 1.0]);
        assert_eq!(
            MeshGradient::new(2, 2, POSITIONS, invalid_colors).unwrap_err(),
            MeshGradientError::NonFinite
        );
    }
}

#[test]
fn mesh_rejects_folded_degenerate_and_inconsistently_wound_cells() {
    for positions in [
        [[0.0, 0.0]; 4],
        [[0.0, 0.0], [100.0, 100.0], [0.0, 100.0], [100.0, 0.0]],
        [[0.0, 0.0], [100.0, 0.0], [0.0, 100.0], [1.0, 1.0]],
        [
            [0.0, 0.0],
            [100.0, 0.0],
            [0.0, 100.0],
            [0.000_000_01, 100.0],
        ],
    ] {
        assert_eq!(
            MeshGradient::new(2, 2, positions, colors()).unwrap_err(),
            MeshGradientError::InvalidTopology
        );
    }
    let positions = [
        [0.0, 0.0],
        [1.0, 0.0],
        [0.0, 0.0],
        [0.0, 1.0],
        [1.0, 1.0],
        [0.0, 1.0],
    ];
    assert!(MeshGradient::new(3, 2, positions, [Color::WHITE; 6]).is_err());
}

#[test]
fn mesh_accepts_skewed_and_reflected_quads_and_preserves_encoding() {
    for positions in [
        POSITIONS,
        POSITIONS.map(|[x, y]| [-x, y]),
        [[20.0, 0.0], [100.0, 10.0], [0.0, 90.0], [80.0, 100.0]],
    ] {
        for interpolation in [MeshInterpolation::Bilinear, MeshInterpolation::Smooth] {
            let brush = Brush::Mesh(
                MeshGradient::new(2, 2, positions, colors())
                    .unwrap()
                    .with_interpolation(interpolation),
            );
            let mut blob = vec![0xdead_beef; 13];
            let (offset, length) = push_encoded_brush(&mut blob, &brush);
            assert_eq!(length as usize, encoded_brush_word_len(&brush));
            let decoded = decode_encoded_brush(&blob, offset, length).unwrap();
            let mut copy = vec![0xdead_beef; 13];
            assert_eq!(push_encoded_brush(&mut copy, &decoded), (offset, length));
            assert_eq!(copy, blob);
            assert!(decode_encoded_brush(&blob, offset, length - 1).is_none());
        }
    }
}

#[test]
fn mesh_spatial_bins_cover_shared_edges_without_searching_all_patches() {
    let n = 12;
    let positions: Vec<_> = (0..n)
        .flat_map(|y| (0..n).map(move |x| [x as f32 * 10.0, y as f32 * 10.0]))
        .collect();
    let mesh = MeshGradient::new(n, n, positions, vec![Color::WHITE; n * n]).unwrap();
    let bin_count = (mesh.bins[0] * mesh.bins[1]) as usize;
    for index in 0..bin_count {
        let begin = mesh.payload[index] as usize;
        let end = mesh.payload[index + 1] as usize;
        assert!(begin < end && end <= mesh.patch_base as usize);
        assert!(end - begin <= 9);
        assert!(mesh.payload[begin..end].contains(&(index as u32)));
    }
}

#[test]
fn mesh_rejects_globally_wrapped_boundary_despite_convex_local_cells() {
    let columns = 17;
    let positions: Vec<_> = (0..2)
        .flat_map(|row| {
            (0..columns).map(move |x| {
                let angle = x as f32 * std::f32::consts::TAU / 12.0;
                let radius = 10.0 + row as f32;
                [radius * angle.cos(), radius * angle.sin()]
            })
        })
        .collect();
    assert_eq!(
        MeshGradient::new(columns, 2, positions, vec![Color::WHITE; columns * 2]).unwrap_err(),
        MeshGradientError::InvalidTopology
    );
}

#[test]
fn mesh_rejects_subnormal_geometry_that_gpu_arithmetic_can_flush_to_zero() {
    let tiny = f32::MIN_POSITIVE * 0.25;
    let positions = [[0.0, 0.0], [tiny, 0.0], [0.0, tiny], [tiny, tiny]];
    assert_eq!(
        MeshGradient::new(2, 2, positions, colors()).unwrap_err(),
        MeshGradientError::InvalidTopology
    );
}

#[test]
fn mesh_rejects_nearly_collapsed_corners_that_amplify_coefficient_rounding() {
    let positions = [[0.0, 0.0], [100.0, 0.0], [0.0, 100.0], [0.0002, 100.0]];
    assert_eq!(
        MeshGradient::new(2, 2, positions, colors()).unwrap_err(),
        MeshGradientError::InvalidTopology
    );
}

#[test]
fn mesh_vertex_grid_can_exceed_spatial_bin_dimensions() {
    let n = 35;
    let positions: Vec<_> = (0..n)
        .flat_map(|y| (0..n).map(move |x| [x as f32, y as f32]))
        .collect();
    let mesh = MeshGradient::new(n, n, positions, vec![Color::WHITE; n * n]).unwrap();
    assert_eq!(mesh.bins, [32, 32]);
    for y in 0..32 {
        for x in 0..32 {
            let bin = y * 32 + x;
            let patch_x = ((x as f64 + 0.5) * (n - 1) as f64 / 32.0).floor() as usize;
            let patch_y = ((y as f64 + 0.5) * (n - 1) as f64 / 32.0).floor() as usize;
            let refs = &mesh.payload[mesh.payload[bin] as usize..mesh.payload[bin + 1] as usize];
            assert!(refs.contains(&((patch_y * (n - 1) + patch_x) as u32)));
        }
    }
}
