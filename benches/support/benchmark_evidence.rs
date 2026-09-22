use sha2::{Digest, Sha256};
use std::path::Path;

pub fn matches_filter(name: &str, filter: Option<&str>) -> bool {
    filter.is_none_or(|filter| {
        filter
            .split(',')
            .filter(|part| !part.is_empty())
            .any(|part| name.starts_with(part))
    })
}

pub fn selected(name: &str) -> bool {
    matches_filter(name, std::env::var("TILEINK_COMPARE_CASE").ok().as_deref())
}

// Compare raw RGBA, including transparent RGB, outside every timing interval.
pub fn capture(image: &tileink::Image, output: &Path, name: &str, phase: usize) -> String {
    let bytes = bytemuck::cast_slice::<u32, u8>(&image.pixels);
    let filename = format!("{name}-{phase}.rgba");
    if let Some(reference) = std::env::var_os("TILEINK_COMPARE_REFERENCE") {
        if std::fs::read(Path::new(&reference).join(&filename)).unwrap() != bytes {
            std::fs::write(output.join(&filename), bytes).unwrap();
            panic!("pixel mismatch: {filename}; actual bytes saved in output");
        }
    } else {
        std::fs::write(output.join(&filename), bytes).unwrap();
    }
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
