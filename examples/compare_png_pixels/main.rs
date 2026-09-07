//! Check regenerated PNGs against a Git commit without rewriting either image.
//! PNG encoders can change IDAT bytes without changing pixels, so binary diffs alone
//! cannot identify rendering regressions. This tool compares decoded samples instead.

mod git;
mod pixels;

#[cfg(test)]
mod tests;

use std::{fs, io::Write, path::Path, process::ExitCode};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Default, PartialEq)]
struct Summary {
    byte_equal: usize,
    pixel_equal: usize,
    failed: usize,
}

fn check(root: &Path, base: &str, report: &mut impl Write) -> Result<Summary> {
    let revision = String::from_utf8(git::command(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ],
    )?)?;
    let revision = revision.trim();
    let files = git::files(root, revision)?;
    if files.is_empty() {
        return Err("no PNG files found; nothing was checked".into());
    }
    writeln!(report, "Baseline: {revision} ({base:?})")?;
    let mut blobs = git::Blobs::new(root)?;
    let mut summary = Summary::default();
    for (path, oid) in files {
        let Some(oid) = oid else {
            summary.failed += 1;
            writeln!(report, "NO_BASELINE\t{path:?}")?;
            continue;
        };
        let after = match fs::read(root.join(&path)) {
            Ok(bytes) => bytes,
            Err(error) => {
                summary.failed += 1;
                writeln!(report, "UNREADABLE\t{path:?}\t{error}")?;
                continue;
            }
        };
        let before = blobs.read(&oid)?;
        match pixels::compare(&before, &after) {
            Ok(None) => {
                let status = if before == after {
                    summary.byte_equal += 1;
                    "SAME_BYTES"
                } else {
                    summary.pixel_equal += 1;
                    "SAME_PIXELS"
                };
                writeln!(report, "{status}\t{path:?}")?;
            }
            Ok(Some(difference)) => {
                summary.failed += 1;
                writeln!(report, "DIFF\t{path:?}\t{difference:?}")?;
            }
            Err(error) => {
                summary.failed += 1;
                writeln!(report, "DECODE_ERROR\t{path:?}\t{error}")?;
            }
        }
    }
    writeln!(
        report,
        "Summary: {} PNGs; {} byte-identical; {} pixel-identical with different encoding/metadata; {} failed",
        summary.byte_equal + summary.pixel_equal + summary.failed,
        summary.byte_equal,
        summary.pixel_equal,
        summary.failed,
    )?;
    writeln!(
        report,
        "Result: {}",
        if summary.failed == 0 { "PASS" } else { "FAIL" }
    )?;
    Ok(summary)
}

fn run() -> Result<Summary> {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| "HEAD".to_owned());
    if args.next().is_some() {
        return Err("usage: cargo run --release --example compare_png_pixels -- [BASE_REF]".into());
    }
    check(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &base,
        &mut std::io::stdout().lock(),
    )
}

fn main() -> ExitCode {
    match run() {
        Ok(summary) => ExitCode::from(u8::from(summary.failed != 0)),
        Err(error) => {
            eprintln!("PNG check failed: {error}");
            ExitCode::from(2)
        }
    }
}
