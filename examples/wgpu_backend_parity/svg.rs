use super::{Result, common, evidence};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Parse each SVG once, sharing immutable trees and decoded resources between
/// reference routes. Track actual file dependencies, including ones outside Git.
pub struct Corpus {
    pub trees: Vec<usvg::Tree>,
    pub resources: Vec<PathBuf>,
    pub snapshot: Value,
    font_directory: PathBuf,
    fonts: Vec<PathBuf>,
}

#[derive(Default)]
struct TrackedResources {
    files: BTreeMap<PathBuf, Option<String>>,
    error: Option<String>,
}

impl TrackedResources {
    fn record(&mut self, path: PathBuf, hash: Option<String>) {
        if self
            .files
            .get(&path)
            .is_some_and(|previous| *previous != hash)
        {
            self.error = Some(format!(
                "resource changed while parsing: {}",
                path.display()
            ));
        }
        self.files.insert(path, hash);
    }
}

impl Corpus {
    pub fn load(inputs: &[PathBuf]) -> Result<Self> {
        let tracked = Arc::new(Mutex::new(TrackedResources::default()));
        let mut fonts = Vec::new();
        let font_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/fonts");
        collect_files(&font_directory, &mut fonts)?;
        fonts.sort();
        for path in &fonts {
            tracked
                .lock()
                .unwrap()
                .record(path.clone(), file_hash(path)?);
        }
        let mut options = common::svg_options();
        let resolver = usvg::ImageHrefResolver::default_string_resolver();
        let resources = Arc::clone(&tracked);
        options.image_href_resolver.resolve_string = Box::new(move |href, options| {
            let path = options.get_abs_path(Path::new(href));
            let before = file_hash(&path);
            let result = resolver(href, options);
            let after = file_hash(&path);
            let mut resources = resources.lock().unwrap();
            match (before, after) {
                (Ok(before), Ok(after)) if before == after => resources.record(path, before),
                _ => {
                    resources.error = Some(format!(
                        "resource changed or became unreadable while parsing: {}",
                        path.display()
                    ))
                }
            }
            result
        });
        let mut trees = Vec::new();
        for input in inputs {
            let data = std::fs::read(input)?;
            tracked
                .lock()
                .unwrap()
                .record(input.clone(), Some(evidence::digest_bytes(&data)));
            options.resources_dir = input.parent().map(Path::to_path_buf);
            trees.push(usvg::Tree::from_data(&data, &options)?);
        }
        drop(options);
        let tracked = Arc::try_unwrap(tracked).ok().unwrap().into_inner().unwrap();
        if let Some(error) = tracked.error {
            return Err(error.into());
        }
        let resources: Vec<_> = tracked.files.keys().cloned().collect();
        let snapshot = serde_json::json!(
            tracked
                .files
                .iter()
                .map(|(path, hash)| serde_json::json!({"path": path, "sha256": hash}))
                .collect::<Vec<_>>()
        );
        let corpus = Self {
            trees,
            resources,
            snapshot,
            font_directory,
            fonts,
        };
        corpus.verify_unchanged()?;
        Ok(corpus)
    }

    pub fn verify_unchanged(&self) -> Result<()> {
        // Font selection also depends on directory membership: hashing only the
        // initially discovered files would miss a font added during the run.
        let mut fonts = Vec::new();
        collect_files(&self.font_directory, &mut fonts)?;
        fonts.sort();
        if fonts != self.fonts || evidence::resource_snapshot(&self.resources)? != self.snapshot {
            return Err("SVG, image, or font resources changed during the run".into());
        }
        Ok(())
    }
}

fn file_hash(path: &Path) -> Result<Option<String>> {
    // usvg intentionally ignores missing image references; record that absence
    // so a later file appearing cannot silently change a reproduced input.
    if path.is_file() {
        Ok(Some(evidence::digest_file(path)?))
    } else {
        Ok(None)
    }
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn added_font_invalidates_the_resource_snapshot() -> Result<()> {
        let dir = std::env::temp_dir().join(format!(
            "tileink-parity-font-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir(&dir)?;
        let corpus = Corpus {
            trees: Vec::new(),
            resources: Vec::new(),
            snapshot: serde_json::json!([]),
            font_directory: dir.clone(),
            fonts: Vec::new(),
        };
        corpus.verify_unchanged()?;
        std::fs::write(dir.join("new.ttf"), b"new font")?;
        let result = corpus.verify_unchanged();
        std::fs::remove_dir_all(dir)?;
        assert!(
            result.is_err(),
            "font directory additions must invalidate the corpus"
        );
        Ok(())
    }

    #[test]
    fn external_image_mutation_changes_the_resource_snapshot() -> Result<()> {
        let dir = std::env::temp_dir().join(format!(
            "tileink-parity-svg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir(&dir)?;
        let input = dir.join("input.svg");
        let image_path = dir.join("external.png");
        super::super::pixels::save_raw(
            &tileink::Image {
                width: 1,
                height: 1,
                pixels: vec![u32::MAX],
            },
            &image_path,
        )?;
        std::fs::write(
            &input,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image href="external.png" width="10" height="10"/></svg>"#,
        )?;
        let corpus = Corpus::load(&[input])?;
        let before = super::super::evidence::resource_snapshot(&corpus.resources)?;
        std::fs::write(&image_path, b"changed image")?;
        let after = super::super::evidence::resource_snapshot(&corpus.resources)?;
        std::fs::remove_dir_all(dir)?;
        assert_ne!(
            before, after,
            "external images outside Git must be part of the fixed inputs"
        );
        Ok(())
    }
}
