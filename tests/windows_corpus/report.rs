use super::{Result, evidence};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub struct Report {
    root: PathBuf,
    expected: BTreeSet<(String, usize)>,
    seen: BTreeSet<(String, usize)>,
    value: Value,
    finished: bool,
}
impl Report {
    pub fn new(root: &Path, cases: &[String], variants: usize, manifest: Value) -> Result<Self> {
        let expected: BTreeSet<_> = cases
            .iter()
            .flat_map(|case| (0..variants).map(move |variant| (case.clone(), variant)))
            .collect();
        if cases.is_empty() || variants == 0 || expected.len() != cases.len() * variants {
            return Err("empty or duplicate acceptance cases".into());
        }
        let report = Self {
            root: root.into(),
            expected,
            seen: BTreeSet::new(),
            finished: false,
            value: json!({
                "schema":1,"passed":false,"manifest":manifest,"cases":cases,"variants":variants,"rows":[],
            }),
        };
        report.save()?;
        Ok(report)
    }
    pub fn record(
        &mut self,
        case: &str,
        variant: usize,
        image: &tileink::Image,
        details: Value,
    ) -> Result {
        let key = (case.to_owned(), variant);
        if self.finished || !self.expected.contains(&key) || self.seen.contains(&key) {
            return Err("duplicate, unexpected or finalized acceptance output".into());
        }
        if image.width == 0
            || image.height == 0
            || u64::from(image.width) * u64::from(image.height) != image.pixels.len() as u64
        {
            return Err("invalid RGBA image extent".into());
        }
        let file = format!("{:05}.rgba", self.seen.len());
        let bytes: &[u8] = bytemuck::cast_slice(&image.pixels);
        use std::io::Write;
        std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(self.root.join(&file))?
            .write_all(bytes)?;
        self.value["rows"].as_array_mut().unwrap().push(json!({"case":case,"variant":variant,
            "width":image.width,"height":image.height,"file":file,"sha256":evidence::digest_bytes(bytes),"details":details}));
        self.seen.insert(key);
        // Keep the initial failed report until finalization. Rewriting the entire
        // source manifest for every image adds quadratic filesystem work.
        Ok(())
    }
    pub fn finish(&mut self, result: Result) -> Result {
        if self.finished {
            return Err("acceptance report already finalized".into());
        }
        self.finished = true;
        let result = result.and_then(|()| {
            if self.seen == self.expected {
                Ok(())
            } else {
                Err("missing acceptance outputs".into())
            }
        });
        self.value["passed"] = json!(result.is_ok());
        if let Err(ref error) = result {
            self.value["error"] = json!(error.to_string());
        }
        self.save()?;
        result
    }
    fn save(&self) -> Result {
        // A crashed writer can only leave a rejected incomplete/invalid report.
        std::fs::write(
            self.root.join("report.json"),
            serde_json::to_vec_pretty(&self.value)?,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_rejects_empty_duplicate_missing_and_unexpected_outputs() -> Result {
        let dir = tempfile::tempdir()?;
        assert!(Report::new(dir.path(), &[], 1, json!({})).is_err());
        assert!(Report::new(dir.path(), &["a".into(), "a".into()], 1, json!({})).is_err());
        let mut report = Report::new(dir.path(), &["a".into()], 2, json!({}))?;
        let image = tileink::Image {
            width: 1,
            height: 1,
            pixels: vec![0],
        };
        assert!(report.record("unknown", 0, &image, json!({})).is_err());
        report.record("a", 0, &image, json!({}))?;
        assert!(report.record("a", 0, &image, json!({})).is_err());
        assert!(report.finish(Ok(())).is_err());
        let value: Value = serde_json::from_slice(&std::fs::read(dir.path().join("report.json"))?)?;
        assert_eq!(value["passed"], false);
        assert!(report.record("a", 1, &image, json!({})).is_err());
        assert!(report.finish(Ok(())).is_err());
        Ok(())
    }
}
