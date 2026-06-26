use std::{
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
};

use tileink::Image;

pub fn example_output(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("examples")
        .join(format!("{name}.png"))
}

pub fn save_image(image: &Image, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut png_writer = encoder.write_header()?;
    let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_le_bytes());
    }
    png_writer.write_image_data(&rgba)?;

    Ok(())
}
