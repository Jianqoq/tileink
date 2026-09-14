use super::Result;
use peniko::{
    Color,
    kurbo::{Affine, Circle, Rect, Shape},
};
use std::path::{Path, PathBuf};
use tileink::{Canvas, FillRule, Radius};

pub fn collect_svgs(input: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let metadata = std::fs::symlink_metadata(input)?;
    if metadata.file_type().is_symlink() {
        return Err(format!("symlink input is not a fixed corpus: {}", input.display()).into());
    }
    if metadata.is_file() {
        if input
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
        {
            paths.push(input.to_path_buf());
        }
    } else if metadata.is_dir() {
        for entry in std::fs::read_dir(input)? {
            collect_svgs(&entry?.path(), paths)?;
        }
    } else {
        return Err(format!(
            "input is not a regular file or directory: {}",
            input.display()
        )
        .into());
    }
    Ok(())
}

pub fn svg_case(index: usize, path: &Path) -> String {
    format!("{index:04}-{}", path.file_stem().unwrap().to_string_lossy())
}

pub fn smoke_scenes() -> Vec<(&'static str, Canvas)> {
    let empty = Canvas::new(17, 15, 1.0);
    let mut rect = Canvas::new(33, 31, 1.0);
    rect.push_rect(
        Rect::new(0.0, 0.0, 33.0, 31.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    rect.push_rect(
        Rect::new(1.25, 0.75, 29.5, 25.125),
        Radius::all(4.25),
        Color::from_rgba8(37, 143, 93, 230),
    );
    let mut path = Canvas::new(65, 49, 1.0);
    path.push_path(
        Circle::new((31.25, 21.75), 19.125).to_path(0.1),
        Color::from_rgba8(45, 111, 211, 220),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    vec![
        ("empty", empty),
        ("fractional-rounded-rect", rect),
        ("fractional-path", path),
    ]
}
