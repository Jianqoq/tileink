#![cfg(all(windows, feature = "directwrite-reference"))]
//!
//! `TILEINK_TEXT_MATRIX_TIER=pr|daily|deep` selects 240, 5,184, or 93,312
//! dynamically rendered Tileink/DirectWrite comparisons. The default is `pr`.

#[path = "directwrite_text_baseline/matrix.rs"]
mod matrix;
#[path = "directwrite_text_baseline/metrics.rs"]
mod metrics;
#[path = "directwrite_text_baseline/reference.rs"]
mod reference;

use std::{error::Error, fs, path::PathBuf};

use matrix::{MatrixTier, matrix_cases};
use metrics::{cohort_ratio_ranges, compare, contact_row, percentile, stack_rows, write_metrics};
use reference::{DirectWriteReference, TileinkReference};

#[test]
fn tileink_matches_directwrite_across_systematic_matrix() -> Result<(), Box<dyn Error>> {
    let tier = MatrixTier::from_env();
    let cases = matrix_cases(tier);
    let reference = DirectWriteReference::new()?;
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("directwrite-text-baseline")
        .join(tier.name());
    fs::create_dir_all(&output_dir)?;

    let override_embolden = std::env::var("TILEINK_TEXT_EMBOLDEN")
        .ok()
        .and_then(|value| value.parse::<f32>().ok());
    let mut tileink_reference = TileinkReference::new(override_embolden);
    let mut metrics = Vec::with_capacity(cases.len());
    let mut contact_rows = Vec::with_capacity(48);

    for case in &cases {
        let tileink = tileink_reference.render(case);
        let directwrite = reference.render(case)?;
        let (case_metrics, diff) = compare(case, &tileink, &directwrite);

        if case.is_representative_artifact() {
            let id = case.id();
            tileink.save(output_dir.join(format!("{id}-tileink.png")))?;
            directwrite.save(output_dir.join(format!("{id}-directwrite.png")))?;
            diff.save(output_dir.join(format!("{id}-diff.png")))?;
            contact_rows.push(contact_row(&tileink, &directwrite, &diff));
        }
        metrics.push(case_metrics);
    }

    write_metrics(&output_dir, tier, &metrics, override_embolden)?;
    stack_rows(&contact_rows).save(output_dir.join("color-matrix.png"))?;

    let average_core_ratio =
        metrics.iter().map(|item| item.core_ratio).sum::<f64>() / metrics.len() as f64;
    let worst_core_ratio = metrics
        .iter()
        .map(|item| item.core_ratio)
        .fold(f64::INFINITY, f64::min);
    let core_p05 = percentile(&metrics, 0.05, |item| item.core_ratio);
    let core_p95 = percentile(&metrics, 0.95, |item| item.core_ratio);
    let average_rgb_mae =
        metrics.iter().map(|item| item.rgb_mae).sum::<f64>() / metrics.len() as f64;
    let worst_rgb_mae = metrics
        .iter()
        .map(|item| item.rgb_mae)
        .fold(f64::NEG_INFINITY, f64::max);
    let rgb_p95 = percentile(&metrics, 0.95, |item| item.rgb_mae);
    let average_coverage_mae =
        metrics.iter().map(|item| item.coverage_mae).sum::<f64>() / metrics.len() as f64;
    let worst_coverage_mae = metrics
        .iter()
        .map(|item| item.coverage_mae)
        .fold(f64::NEG_INFINITY, f64::max);
    let coverage_p95 = percentile(&metrics, 0.95, |item| item.coverage_mae);
    let maximum_core_ratio = metrics
        .iter()
        .map(|item| item.core_ratio)
        .fold(f64::NEG_INFINITY, f64::max);
    let ink_ratio = metrics.iter().map(|item| item.tile_ink).sum::<f64>()
        / metrics.iter().map(|item| item.reference_ink).sum::<f64>();
    let worst_fringe_excess = metrics
        .iter()
        .map(|item| item.tile_fringe - item.reference_fringe)
        .fold(f64::NEG_INFINITY, f64::max);
    let cohort_ranges = cohort_ratio_ranges(&metrics);
    let thresholds = tier.thresholds();

    eprintln!(
        "DirectWrite {tier:?} matrix ({} cases): core avg={average_core_ratio:.3}, p05/p95={core_p05:.3}/{core_p95:.3}, range={worst_core_ratio:.3}..{maximum_core_ratio:.3}, ink={ink_ratio:.3}, \
         RGB MAE avg/p95/worst={average_rgb_mae:.4}/{rgb_p95:.4}/{worst_rgb_mae:.4}, coverage MAE avg/p95/worst={average_coverage_mae:.4}/{coverage_p95:.4}/{worst_coverage_mae:.4}, \
         worst fringe excess={worst_fringe_excess:.4}, cohort core={:.3}..{:.3}, cohort ink={:.3}..{:.3}; artifacts={}",
        cases.len(),
        cohort_ranges.core_min,
        cohort_ranges.core_max,
        cohort_ranges.ink_min,
        cohort_ranges.ink_max,
        output_dir.display()
    );
    let mut worst_core_cases = metrics.iter().collect::<Vec<_>>();
    worst_core_cases.sort_by(|left, right| left.core_ratio.total_cmp(&right.core_ratio));
    for item in worst_core_cases.into_iter().take(12) {
        eprintln!(
            "  {:<58} core={:>3}/{:<3} ({:.3}) rgb={:.4} coverage={:.4} fringe={:.4}/{:.4}",
            item.name,
            item.tile_core,
            item.reference_core,
            item.core_ratio,
            item.rgb_mae,
            item.coverage_mae,
            item.tile_fringe,
            item.reference_fringe,
        );
    }

    assert!(
        thresholds.average_core.contains(&average_core_ratio),
        "average core ratio {average_core_ratio:.3} is outside the DirectWrite {:?} baseline",
        tier
    );
    assert!(
        core_p05 >= thresholds.core_p05_min,
        "5th-percentile core ratio {core_p05:.3} is below the DirectWrite baseline"
    );
    assert!(
        core_p95 <= thresholds.core_p95_max,
        "95th-percentile core ratio {core_p95:.3} is above the DirectWrite baseline"
    );
    assert!(
        thresholds.ink.contains(&ink_ratio),
        "ink ratio {ink_ratio:.3} is outside the DirectWrite baseline"
    );
    assert!(
        average_rgb_mae <= 0.14,
        "average RGB MAE {average_rgb_mae:.4} exceeds the DirectWrite baseline"
    );
    assert!(
        rgb_p95 <= 0.28,
        "95th-percentile RGB MAE {rgb_p95:.4} exceeds the DirectWrite baseline"
    );
    assert!(
        average_coverage_mae <= thresholds.average_coverage_mae_max,
        "average coverage MAE {average_coverage_mae:.4} exceeds the DirectWrite baseline"
    );
    assert!(
        coverage_p95 <= thresholds.coverage_p95_max,
        "95th-percentile coverage MAE {coverage_p95:.4} exceeds the DirectWrite baseline"
    );
    assert!(
        worst_fringe_excess <= thresholds.fringe_excess_max,
        "Tileink fringe exceeds DirectWrite by {worst_fringe_excess:.4}"
    );
    assert!(
        thresholds.cohort_core.contains(&cohort_ranges.core_min)
            && thresholds.cohort_core.contains(&cohort_ranges.core_max),
        "foreground/background cohort core range {:.3}..{:.3} is outside the DirectWrite baseline",
        cohort_ranges.core_min,
        cohort_ranges.core_max,
    );
    assert!(
        thresholds.cohort_ink.contains(&cohort_ranges.ink_min)
            && thresholds.cohort_ink.contains(&cohort_ranges.ink_max),
        "foreground/background cohort ink range {:.3}..{:.3} is outside the DirectWrite baseline",
        cohort_ranges.ink_min,
        cohort_ranges.ink_max,
    );

    Ok(())
}

#[test]
fn systematic_matrix_tiers_have_the_documented_case_counts() {
    assert_eq!(matrix_cases(MatrixTier::Pr).len(), 240);
    assert_eq!(matrix_cases(MatrixTier::Daily).len(), 5_184);
    assert_eq!(matrix_cases(MatrixTier::Deep).len(), 93_312);
}
