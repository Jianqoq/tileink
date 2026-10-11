//! Complete paint kernels run before fine compositing. Only the affected draw
//! rectangle gets a texture; ordinary brushes retain their existing fast path.
use super::{Output, Result, record_brush};
use crate::native::runtime::{
    compute::{ComputeBatch, Resource, ResourceId},
    program::scene::SceneImages,
};
use crate::shared::{
    affine::GpuAffine,
    bounds::Bounds,
    brush::{ENCODED_BRUSH_HEADER_WORDS, decode_encoded_brush},
    draw_record::DrawRecord,
    filter_config::FilterConfig,
    gpu_layout::brush::{
        GPU_BRUSH_PATTERN_RESOURCE, GPU_BRUSH_PROGRAM, GPU_RESOURCE_TEXTURE_PLACEMENT_BIT,
    },
};
use crate::{Brush, Canvas};
use std::collections::BTreeMap;

pub(crate) struct BrushImages {
    start: usize,
    textures: Vec<ResourceId>,
}
impl BrushImages {
    pub(crate) fn attach(
        &self,
        batch: &mut ComputeBatch,
        images: &SceneImages,
    ) -> Result<SceneImages> {
        let Resource::TextureTable(table) = &batch.resources()[images.table.index()] else {
            return Err("invalid native image table".into());
        };
        let mut table = table.clone();
        if self.start + self.textures.len() > table.len() {
            return Err("native shader brushes exceed texture table capacity".into());
        }
        table[self.start..self.start + self.textures.len()].copy_from_slice(&self.textures);
        Ok(SceneImages {
            atlas: images.atlas,
            table: batch.texture_table(&table)?,
            sampler: images.sampler,
        })
    }
}
pub(crate) struct PreparedBrushes {
    pub(crate) draws: Vec<DrawRecord>,
    pub(crate) blob: Vec<u32>,
    pub(crate) images: BrushImages,
}

fn raster(
    batch: &mut ComputeBatch,
    brush: &crate::ProgramBrush,
    affine: GpuAffine,
    size: [u32; 2],
    bounds: Bounds,
    index: usize,
) -> Result<(ResourceId, [u32; ENCODED_BRUSH_HEADER_WORDS])> {
    if index >= crate::shared::gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY as usize {
        return Err("native shader brushes exceed texture table capacity".into());
    }
    let bounds = bounds.intersect(Bounds::canvas(size[0], size[1]));
    let extent = [bounds.width().max(1), bounds.height().max(1)];
    let origin = [bounds.x0.max(0) as u32, bounds.y0.max(0) as u32];
    let texture = match batch.reusable_overwritten_surface(extent)? {
        Some(texture) => texture,
        None => {
            batch.texture_rgba8(extent, vec![0; extent[0] as usize * extent[1] as usize * 4])?
        }
    };
    if !bounds.is_empty() {
        let mut instance = brush.0.clone();
        let [a, b, c, d, e, f] = instance.transform;
        let inverse = GpuAffine { a, b, c, d, e, f }.compose(
            affine
                .inverse()
                .ok_or("native brush transform is singular")?,
        );
        instance.transform = [
            inverse.a, inverse.b, inverse.c, inverse.d, inverse.e, inverse.f,
        ];
        record_brush(
            batch,
            &instance,
            FilterConfig {
                width: size[0],
                height: size[1],
                region_x0: origin[0],
                region_y0: origin[1],
                region_width: bounds.width(),
                region_height: bounds.height(),
                ..Default::default()
            },
            None,
            None,
            Output {
                image: texture,
                origin,
                extent,
            },
        )?;
    }
    let [w, h] = extent.map(|n| n as f32);
    let mut header = [0; ENCODED_BRUSH_HEADER_WORDS];
    header[0] = GPU_BRUSH_PATTERN_RESOURCE;
    header[4] = GPU_RESOURCE_TEXTURE_PLACEMENT_BIT | index as u32;
    header[5] = extent[0];
    header[6] = extent[1];
    header[7] = 255;
    for (word, value) in header[9..15].iter_mut().zip([
        affine.a / w,
        affine.b / h,
        affine.c / w,
        affine.d / h,
        (affine.e - origin[0] as f32) / w,
        (affine.f - origin[1] as f32) / h,
    ]) {
        *word = value.to_bits();
    }
    Ok((texture, header))
}

pub(crate) fn prepare_brushes(
    batch: &mut ComputeBatch,
    canvas: &Canvas,
    images: Option<&crate::shared::image_resource::GpuImageResourceUpload>,
) -> Result<PreparedBrushes> {
    let start = images.map_or(0, |i| i.textures().len());
    let mut textures = Vec::new();
    let mut draws = canvas.draw_records.clone();
    let mut blob = Vec::new();
    let mut offsets = BTreeMap::new();
    for draw in &mut draws {
        if draw.brush_offset == DrawRecord::NONE {
            continue;
        }
        let program = canvas.brush_blob.get(draw.brush_offset as usize) == Some(&GPU_BRUSH_PROGRAM);
        let transform = draw.transform;
        let key = (
            draw.brush_offset,
            if program {
                [
                    transform.a,
                    transform.b,
                    transform.c,
                    transform.d,
                    transform.e,
                    transform.f,
                ]
                .map(f32::to_bits)
            } else {
                [0; 6]
            },
            if program {
                [
                    draw.pixel_bounds.x0,
                    draw.pixel_bounds.y0,
                    draw.pixel_bounds.x1,
                    draw.pixel_bounds.y1,
                ]
            } else {
                [0; 4]
            },
        );
        if let Some(&(offset, len)) = offsets.get(&key) {
            draw.brush_offset = offset;
            draw.brush_len = len;
            continue;
        }
        let offset = blob.len() as u32;
        if program {
            let Brush::Program(brush) =
                decode_encoded_brush(&canvas.brush_blob, draw.brush_offset, draw.brush_len)
                    .ok_or("invalid native brush source record")?
            else {
                return Err("native brush record kind mismatch".into());
            };
            let b = draw.pixel_bounds;
            let (texture, header) = raster(
                batch,
                &brush,
                transform,
                [canvas.physical_width(), canvas.physical_height()],
                Bounds::new(b.x0, b.y0, b.x1, b.y1),
                start + textures.len(),
            )?;
            textures.push(texture);
            blob.extend(header);
        } else {
            let range =
                draw.brush_offset as usize..draw.brush_offset as usize + draw.brush_len as usize;
            blob.extend(
                canvas
                    .brush_blob
                    .get(range)
                    .ok_or("invalid native brush record bounds")?,
            );
        }
        let len = blob.len() as u32 - offset;
        if !program {
            crate::shared::gpu_brush::GpuBrushUpload::patch_scene_brush_blob(
                &mut blob,
                &[DrawRecord {
                    brush_offset: offset,
                    brush_len: len,
                    ..*draw
                }],
                images,
            );
        }
        offsets.insert(key, (offset, len));
        draw.brush_offset = offset;
        draw.brush_len = len;
    }
    Ok(PreparedBrushes {
        draws,
        blob,
        images: BrushImages { start, textures },
    })
}

pub(crate) fn brushes_for_filter(
    batch: &mut ComputeBatch,
    blob: &[u32],
    offsets: &[u32],
    start: usize,
    config: FilterConfig,
    images: &SceneImages,
) -> Result<(Option<ResourceId>, SceneImages)> {
    let base = config.brush_offset as usize;
    if blob.get(base) != Some(&GPU_BRUSH_PROGRAM) {
        return Ok((
            None,
            SceneImages {
                atlas: images.atlas,
                table: images.table,
                sampler: images.sampler,
            },
        ));
    }
    let length = blob
        .get(base + 2)
        .zip(blob.get(base + 3))
        .and_then(|(a, b)| a.checked_add(*b))
        .ok_or("invalid native filter brush record")?;
    let Brush::Program(brush) = decode_encoded_brush(blob, config.brush_offset, length)
        .ok_or("invalid native filter brush source")?
    else {
        return Err("native filter brush kind mismatch".into());
    };
    if offsets.binary_search(&config.brush_offset).is_err() {
        return Err("native filter brush offset is not a record start".into());
    }
    let bounds = Bounds::new(
        config.region_x0 as i32,
        config.region_y0 as i32,
        (config.region_x0 + config.region_width) as i32,
        (config.region_y0 + config.region_height) as i32,
    );
    let (texture, header) = raster(
        batch,
        &brush,
        GpuAffine::IDENTITY,
        [config.width, config.height],
        bounds,
        start,
    )?;
    let images = BrushImages {
        start,
        textures: vec![texture],
    }
    .attach(batch, images)?;
    let mut patched = blob.to_vec();
    patched[base..base + header.len()].copy_from_slice(&header);
    Ok((
        Some(batch.buffer(bytemuck::cast_slice(&patched).to_vec())?),
        images,
    ))
}
