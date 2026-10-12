use super::*;
use crate::shared::gpu_text::{GlyphImageRecord, GlyphRecord, GlyphRunRecord};

// Typed current metadata for the shared 8x8 bitmap; coarse does not read texels.
pub(super) fn bitmap_text(xs: &[i32]) -> Vec<u32> {
    let run = GlyphRunRecord {
        glyph_start: 0,
        glyph_count: xs.len() as u32,
    };
    let mut words = bytemuck::cast_slice(&[run]).to_vec();
    for &x in xs {
        words.extend_from_slice(bytemuck::cast_slice(&[GlyphRecord {
            image_id: 0,
            x,
            y: 0,
        }]));
    }
    words.extend_from_slice(bytemuck::cast_slice(&[GlyphImageRecord {
        width: 8,
        height: 8,
        ..Default::default()
    }]));
    words
}

pub(super) fn glyph_page_scene(linked: bool, bins: bool) -> CountScene {
    let mut scene = count_scene(257, linked, 0);
    let capacity = 1 + 257 * 3;
    scene.reserve_streams(260, capacity);
    let paint = super::emit_scene::paint_scene(14, false);
    scene.draws = paint.draws;
    scene.sdf = paint.sdf;
    scene.text = paint.text;
    scene.config[10] = 1;
    scene.config[11] = 3;
    scene.config[14] = 12;
    scene.config[15] = 1;
    let index_base = 6 + 260 * 6 + capacity as usize + 2;
    for index in 0..257usize {
        let word = if linked {
            index / 256 * 257 + 1 + index % 256
        } else {
            index
        };
        scene.work[index_base + word] = 0;
    }
    scene.work[..6].copy_from_slice(&[258, 1, 259, 257 * 3, 1, 1 + 257 * 3]);
    scene.expected = scene.work.clone();
    for index in 0..257usize {
        let glyph = 1 + index * 3;
        scene.expected[12 + index * 6..18 + index * 6].copy_from_slice(&[
            10,
            0,
            0,
            glyph as u32,
            glyph as u32 + 3,
            0,
        ]);
        let base = 6 + 260 * 6 + glyph;
        scene.expected[base] = 0;
        scene.expected[base + 1] = 1;
        scene.expected[base + 2] = 2;
    }
    scene.expected[12 + 257 * 6..18 + 257 * 6].fill(0);
    if bins {
        scene.expected[scene.kind_base] = 0;
    }
    scene
}

#[test]
fn glyph_fixture_oracles_include_bilinear_border_candidates() {
    for mode in 14..=16 {
        let scene = super::emit_scene::paint_scene(mode, false);
        // x=16 still overlaps the tile through the one-pixel sampling guard.
        assert_eq!(&scene.expected[15..17], &[1, 4]);
        assert_eq!(scene.work[3], 3);
        let base = 6 + scene.config[7] as usize * 6;
        match mode {
            14 => assert_eq!(&scene.expected[base + 1..base + 4], &[0, 1, 2]),
            15 => assert_eq!(&scene.expected[base + 1..base + 4], &[0x4545_4545; 3]),
            16 => assert_eq!(scene.expected[base + 1], 0),
            _ => unreachable!(),
        }
    }
    for linked in [false, true] {
        let scene = glyph_page_scene(linked, false);
        assert_eq!(scene.work[3], 257 * 3);
        assert_eq!(scene.work[5], 1 + 257 * 3);
        for index in 0..257 {
            let glyph = 1 + index * 3;
            assert_eq!(
                &scene.expected[15 + index * 6..17 + index * 6],
                &[glyph as u32, glyph as u32 + 3]
            );
        }
    }
}
