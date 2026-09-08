use super::{Result, evidence, pixels};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use tileink::Image;

/// The manifest fixes the expected frames/routes before rendering. Completion is
/// impossible if a frame is missing, duplicated, or has the wrong output count.
pub struct Report {
    output: PathBuf,
    expected: BTreeSet<String>,
    routes: Vec<String>,
    metadata: Vec<Value>,
    frames: BTreeMap<String, Value>,
    failed: usize,
}

impl Report {
    pub fn new(output: &Path, cases: &[String], metadata: Vec<Value>) -> Result<Self> {
        let expected: BTreeSet<_> = cases.iter().cloned().collect();
        let routes: Vec<_> = metadata
            .iter()
            .map(|route| {
                route["route"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("missing route name")
            })
            .collect::<std::result::Result<_, _>>()?;
        if expected.is_empty() || expected.len() != cases.len() {
            return Err("empty or duplicate case manifest".into());
        }
        if routes.len() < 2 || routes.iter().collect::<BTreeSet<_>>().len() != routes.len() {
            return Err("missing or duplicate route manifest".into());
        }
        let report = Self {
            output: output.to_path_buf(),
            expected,
            routes,
            metadata,
            frames: BTreeMap::new(),
            failed: 0,
        };
        report.checkpoint(false, None)?;
        Ok(report)
    }

    pub fn record(&mut self, case: &str, images: &[Image]) -> Result<bool> {
        if !self.expected.contains(case) || self.frames.contains_key(case) {
            return Err(format!("unexpected or duplicate frame {case}").into());
        }
        if images.len() != self.routes.len() {
            return Err(format!("{case}: missing route output").into());
        }
        let mut comparisons = Vec::new();
        let mut equal = true;
        for (index, image) in images.iter().enumerate().skip(1) {
            let difference = pixels::compare(&images[0], image)?;
            equal &= difference.pixels == 0;
            if difference.pixels != 0 {
                println!(
                    "{case}: {} vs {}: {difference:?}",
                    self.routes[0], self.routes[index]
                );
                pixels::save_diff(
                    &images[0],
                    image,
                    &self
                        .output
                        .join("diff")
                        .join(&self.routes[index])
                        .join(format!("{case}.png")),
                )?;
            }
            comparisons.push(
                json!({"against": self.routes[index], "different_pixels": difference.pixels,
                "max_channel_delta": difference.max_channel_delta, "first": difference.first}),
            );
        }
        let mut outputs = Vec::new();
        for (route, image) in self.routes.iter().zip(images) {
            let relative = Path::new(route).join(format!("{case}.png"));
            let path = self.output.join(&relative);
            pixels::save_raw(image, &path)?;
            outputs.push(json!({"route": route, "path": relative, "png_sha256": evidence::digest_file(&path)?}));
        }
        self.frames.insert(
            case.to_owned(),
            json!({"case": case, "width": images[0].width, "height": images[0].height,
            "reference": self.routes[0], "outputs": outputs, "comparisons": comparisons}),
        );
        self.failed += usize::from(!equal);
        self.checkpoint(false, None)?;
        Ok(equal)
    }

    pub fn finish(&self) -> Result<()> {
        if self.frames.len() != self.expected.len() {
            return Err(format!(
                "incomplete run: {} / {} frames",
                self.frames.len(),
                self.expected.len()
            )
            .into());
        }
        self.checkpoint(true, None)?;
        if self.failed > 0 {
            return Err(format!("{} frames failed exact backend parity", self.failed).into());
        }
        Ok(())
    }

    pub fn fail(&self, error: &str) -> Result<()> {
        self.checkpoint(false, Some(error))
    }

    fn checkpoint(&self, complete: bool, error: Option<&str>) -> Result<()> {
        let report = json!({"schema": 1, "complete": complete, "passed": complete && self.failed == 0,
            "expected_frames": self.expected.len(), "completed_frames": self.frames.len(),
            "mismatched_frames": self.failed, "routes": self.metadata, "frames": self.frames.values().collect::<Vec<_>>(), "error": error});
        std::fs::write(
            self.output.join("report.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_duplicate_and_wrong_route_outputs_cannot_pass() -> Result<()> {
        let output = std::env::temp_dir().join(format!(
            "tileink-parity-report-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir(&output)?;
        let metadata = vec![json!({"route": "dx12"}), json!({"route": "vulkan"})];
        assert!(Report::new(&output, &[], metadata.clone()).is_err());
        assert!(Report::new(&output, &["a".into(), "a".into()], metadata.clone()).is_err());
        assert!(Report::new(&output, &["a".into()], vec![metadata[0].clone(); 2]).is_err());
        let mut report = Report::new(&output, &["a".into(), "b".into()], metadata)?;
        let image = Image {
            width: 1,
            height: 1,
            pixels: vec![0],
        };
        let images = [image.clone(), image];
        assert!(report.finish().is_err());
        assert!(report.record("a", &images[..1]).is_err());
        assert!(report.record("unknown", &images).is_err());
        report.record("a", &images)?;
        assert!(report.record("a", &images).is_err());
        assert!(report.finish().is_err());
        report.record("b", &images)?;
        report.finish()?;
        let value: Value = serde_json::from_slice(&std::fs::read(output.join("report.json"))?)?;
        assert_eq!(value["passed"], true);
        std::fs::remove_dir_all(output)?;
        Ok(())
    }
}
