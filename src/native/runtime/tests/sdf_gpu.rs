//! Native SDF verification uses analytic geometry and coordinate invariants,
//! without generated artifacts from a retired renderer.
use super::{Result, compute::ComputeBatch, gpu_support::context};
use crate::shared::{
    affine::GpuAffine,
    filter_config::FilterConfig,
    gpu_constants::{SDF_PROBE_AFFINE_WORD, SDF_PROBE_REQUEST_WORDS, SDF_RECORD_WORDS},
    gpu_sdf::{EncodedSdf, encode_sdf, encode_sdf_shadow},
};
use crate::*;
use peniko::kurbo::{Affine, Point};

#[path = "sdf/cases.rs"]
mod cases;
#[path = "sdf/equivalence.rs"]
mod equivalence;
#[path = "sdf/oracle.rs"]
mod oracle;
#[path = "sdf/scales.rs"]
mod scales;

#[derive(Clone, Copy, Debug)]
struct Request {
    record: u32,
    point: [f32; 2],
    inverse: GpuAffine,
}

fn points() -> Vec<[f32; 2]> {
    [0.0, 0.5]
        .into_iter()
        .flat_map(|phase| {
            (-4..=20)
                .flat_map(move |y| (-4..=20).map(move |x| [x as f32 + phase, y as f32 + phase]))
        })
        .collect()
}

fn inverses() -> [GpuAffine; 4] {
    [
        Affine::IDENTITY,
        Affine::scale_non_uniform(0.5, 2.0),
        Affine::new([1.0, 0.25, 0.5, 1.0, -2.0, 1.0]),
        Affine::new([0.75, 0.5, -0.5, 0.75, 3.25, -2.5]),
    ]
    .map(|a| GpuAffine::from_logical(a, 1.0))
}

fn sample(
    context: &NativeContext,
    records: &[EncodedSdf],
    requests: &[Request],
) -> Result<Vec<u32>> {
    let paint: Vec<_> = records
        .iter()
        .flat_map(|s| {
            std::iter::once(s.kind).chain(
                s.coords
                    .into_iter()
                    .chain(s.radii)
                    .chain(s.stroke)
                    .chain(s.shadow)
                    .map(f32::to_bits),
            )
        })
        .collect();
    let words: Vec<_> = requests
        .iter()
        .flat_map(|r| {
            let a = r.inverse;
            let mut words = [0; SDF_PROBE_REQUEST_WORDS as usize];
            words[..3].copy_from_slice(&[
                r.record * SDF_RECORD_WORDS,
                r.point[0].to_bits(),
                r.point[1].to_bits(),
            ]);
            let base = SDF_PROBE_AFFINE_WORD as usize;
            words[base..base + 6]
                .copy_from_slice(&[a.a, a.b, a.c, a.d, a.e, a.f].map(f32::to_bits));
            words
        })
        .collect();
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(
        bytemuck::bytes_of(&FilterConfig {
            pixel_count: requests.len() as u32,
            ..Default::default()
        })
        .to_vec(),
    )?;
    let paint = batch.buffer(bytemuck::cast_slice(&paint).to_vec())?;
    let positions = batch.buffer(bytemuck::cast_slice(&words).to_vec())?;
    let sentinel = 0xa1b2_c3d4;
    let output =
        batch.buffer(bytemuck::cast_slice(&vec![sentinel; requests.len() + 4]).to_vec())?;
    // SAFETY: bounded paint/requests are owned by the batch. Extra invocations
    // and trailing sentinel words verify the production logical-count guard.
    unsafe {
        batch.dispatch(
            "sdf_coverage_words",
            &[(0, config), (5, positions), (6, output), (7, paint)],
            [requests.len().div_ceil(256) as u32 + 1, 1, 1],
        )?;
    }
    batch.readback(output)?;
    let mut first = Vec::new();
    for repetition in 0..3 {
        let receipt = context
            .adapter
            .submit_compute(&batch)
            .map_err(|error| format!("{error:?}"))?;
        let data = receipt.readback()?.remove(0);
        let actual: Vec<_> = data
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(u32::from_le_bytes)
            .collect();
        assert_eq!(&actual[requests.len()..], &[sentinel; 4]);
        assert!(
            actual[..requests.len()]
                .iter()
                .all(|&coverage| coverage <= 255)
        );
        if repetition == 0 {
            first = actual;
        } else {
            assert_eq!(actual, first);
        }
    }
    first.truncate(requests.len());
    Ok(first)
}

// Independent f64 geometry and reordered affine arithmetic may fall on either
// side of an 8-bit rounding boundary. One coverage code is the numeric budget.
fn assert_close(actual: u32, expected: u32, label: impl std::fmt::Debug) {
    assert!(
        actual.abs_diff(expected) <= 1,
        "{label:?}: actual {actual}, expected {expected}"
    );
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_triangle_edge_coverage_is_invariant_under_tiny_and_large_coordinate_scales() -> Result<()>
{
    let context = context()?;
    for size in [
        1.0e-20_f32,
        1.0e-10,
        1.0e-6,
        1.0e-4,
        1.0e-3,
        1.0,
        1.0e10,
        1.0e20,
    ] {
        let triangle = SdfTriangle::new(
            Point::new(0.0, 0.0),
            Point::new(f64::from(size), 0.0),
            Point::new(0.0, f64::from(size)),
            0.0,
        );
        let inverse = GpuAffine::from_logical(
            Affine::scale_non_uniform(f64::from(size), f64::from(size) * 2.0),
            1.0,
        );
        let requests = [
            Request {
                record: 0,
                point: [0.5, -0.025],
                inverse,
            },
            Request {
                record: 0,
                point: [0.25, 0.05],
                inverse,
            },
        ];
        let actual = sample(&context, &[encode_sdf(Sdf::Triangle(triangle))], &requests)?;
        // The nearest boundary is the horizontal edge. Its signed distances are 0.05s/-0.1s;
        // the transformed unit normal has length 2s, independent of the authored scale.
        for (value, coverage) in actual.into_iter().zip([0.475_f32, 0.55]) {
            assert_close(value, (coverage * 255.0).round() as u32, size);
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_all_kinds_preserve_coverage_under_physical_coordinate_changes() -> Result<()> {
    let context = context()?;
    let records = cases::records();
    let kinds: std::collections::BTreeSet<_> = records.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, (1..=19).collect());
    let points = points();
    let frames = [
        Affine::IDENTITY,
        Affine::translate((16.0, -8.0)),
        Affine::scale_non_uniform(-1.0, 1.0),
        Affine::scale_non_uniform(1.0, -1.0),
        Affine::new([0.0, 1.0, -1.0, 0.0, 4.0, -2.0]),
    ];
    for inverse in inverses() {
        let mut requests = Vec::new();
        for (record, _) in records.iter().enumerate() {
            for frame in frames {
                let composed = inverse.compose(GpuAffine::from_logical(frame, 1.0));
                let to_physical = frame.inverse();
                for &point in &points {
                    let physical =
                        to_physical * Point::new(f64::from(point[0]), f64::from(point[1]));
                    requests.push(Request {
                        record: record as u32,
                        point: [physical.x as f32, physical.y as f32],
                        inverse: composed,
                    });
                }
            }
        }
        let actual = sample(&context, &records, &requests)?;
        for (index, record) in records.iter().enumerate() {
            let start = index * points.len() * frames.len();
            let base = &actual[start..start + points.len()];
            assert!(
                base.iter().min() < base.iter().max(),
                "constant coverage for kind {}",
                record.kind
            );
            for frame in 1..frames.len() {
                for (point, &expected) in base.iter().enumerate() {
                    assert_close(
                        actual[start + frame * points.len() + point],
                        expected,
                        (record.kind, inverse, frame, points[point]),
                    );
                }
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_sdf_absent_unknown_and_out_of_range_records_are_transparent() -> Result<()> {
    let context = context()?;
    let records = [
        EncodedSdf::NONE,
        EncodedSdf {
            kind: u32::MAX,
            ..EncodedSdf::NONE
        },
    ];
    let requests: Vec<_> = [0, 1, 2, u32::MAX / SDF_RECORD_WORDS]
        .into_iter()
        .map(|record| Request {
            record,
            point: [5.0; 2],
            inverse: GpuAffine::IDENTITY,
        })
        .collect();
    assert_eq!(sample(&context, &records, &requests)?, [0; 4]);
    context.check_validation()?;
    Ok(())
}
