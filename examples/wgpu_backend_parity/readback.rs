use super::Result;
use tileink::Image;

/// Read the application target's physical RGBA bytes, excluding only row padding.
#[cfg(any(windows, target_os = "macos"))]
pub fn rgba8(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Result<Image> {
    let size = texture.size();
    if texture.format() != wgpu::TextureFormat::Rgba8Unorm
        || texture.dimension() != wgpu::TextureDimension::D2
        || size.depth_or_array_layers != 1
        || texture.sample_count() != 1
        || !texture.usage().contains(wgpu::TextureUsages::COPY_SRC)
    {
        return Err(
            "readback requires a single-sample, single-layer Rgba8Unorm COPY_SRC texture".into(),
        );
    }
    let row_bytes = u64::from(size.width) * 4;
    let pitch = row_bytes.next_multiple_of(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT));
    let pitch_u32 = u32::try_from(pitch)?;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("exact RGBA application-target readback"),
        size: pitch
            .checked_mul(u64::from(size.height))
            .ok_or("readback size overflow")?,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch_u32),
                rows_per_image: None,
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    receiver.recv()??;
    let view = readback.slice(..).get_mapped_range()?;
    let image = unpack_rows(&view, size.width, size.height, usize::try_from(pitch)?);
    drop(view);
    readback.unmap();
    image
}

fn unpack_rows(bytes: &[u8], width: u32, height: u32, pitch: usize) -> Result<Image> {
    let row_bytes = usize::try_from(width)?
        .checked_mul(4)
        .ok_or("row size overflow")?;
    let rows = usize::try_from(height)?;
    if width == 0 || height == 0 || pitch < row_bytes {
        return Err("invalid readback dimensions or row pitch".into());
    }
    let required = (rows - 1)
        .checked_mul(pitch)
        .and_then(|v| v.checked_add(row_bytes))
        .ok_or("readback span overflow")?;
    if bytes.len() < required {
        return Err("truncated readback rows".into());
    }
    let mut pixels = Vec::with_capacity(
        usize::try_from(width)?
            .checked_mul(rows)
            .ok_or("pixel count overflow")?,
    );
    for row in 0..rows {
        pixels.extend(
            bytes[row * pitch..row * pitch + row_bytes]
                .chunks_exact(4)
                .map(|rgba| u32::from_le_bytes(rgba.try_into().unwrap())),
        );
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_padding_is_removed_without_changing_any_rgba_channel() {
        let bytes = [
            12, 34, 56, 0, 0, 0, 0, 1, 200, 201, 202, 203, 255, 128, 64, 255, 9, 8, 7, 0,
        ];
        let image = unpack_rows(&bytes, 2, 2, 12).unwrap();
        assert_eq!(
            image.pixels,
            [
                [12, 34, 56, 0],
                [0, 0, 0, 1],
                [255, 128, 64, 255],
                [9, 8, 7, 0]
            ]
            .map(u32::from_le_bytes)
        );
    }

    #[test]
    fn invalid_or_incomplete_row_layouts_are_rejected() {
        assert!(unpack_rows(&[0; 19], 2, 2, 12).is_err());
        assert!(unpack_rows(&[0; 32], 2, 2, 7).is_err());
        assert!(unpack_rows(&[], 0, 1, 0).is_err());
        assert!(unpack_rows(&[], 1, 0, 4).is_err());
        assert!(unpack_rows(&[], u32::MAX, u32::MAX, usize::MAX).is_err());
    }
}
