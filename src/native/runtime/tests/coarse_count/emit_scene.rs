use super::*;

pub(super) fn emission_scene(
    draws: u32,
    linked: bool,
    stack: u32,
    capacity: u32,
    bins: bool,
) -> CountScene {
    let mut scene = count_scene(draws, linked, stack);
    scene.reserve_streams(capacity, 3);
    for draw in scene.draws.chunks_exact_mut(31) {
        draw[6] = u32::MAX;
    }
    let mut stream = Vec::new();
    for index in 0..draws {
        let draw = index % 8;
        let record = match draw {
            0 | 1 => [9, 0, 0, draw, 0, draw],
            2 => [1, 1, 0, 0, 0, 2],
            3 => [11, 1, 0, 0, 0, 3],
            6 => [3, 1, 0, 0, 0, 0],
            _ => continue,
        };
        stream.push(record);
    }
    let nonempty = !stream.is_empty();
    if nonempty && stack == 2 {
        stream.insert(0, [5, 1, 0, 0, 0, 0]);
        stream.push([6, 0, 0, 0, 0, 0]);
    }
    if nonempty {
        stream.push([0; 6]);
    }
    scene.work[0] = stream.len() as u32;
    scene.work[1] = 1;
    scene.work[2] = 1 + stream.len() as u32;
    scene.work[3] = 0;
    scene.work[4] = 1;
    scene.work[5] = 1;
    scene.expected = scene.work.clone();
    if stack != 3 {
        for (index, record) in stream.iter().enumerate() {
            let dst = index + 1;
            if dst < capacity as usize {
                scene.expected[6 + dst * 6..6 + (dst + 1) * 6].copy_from_slice(record);
            }
        }
    }
    if bins {
        scene.expected[scene.kind_base] = u32::from(!nonempty);
    }
    scene
}

pub(super) fn paint_scene(mode: u32, bins: bool) -> CountScene {
    let mut scene = count_scene(1, false, 0);
    scene.reserve_streams(8, if mode == 16 { 2 } else { 8 });
    for draw in scene.draws.chunks_exact_mut(31) {
        draw[6] = u32::MAX;
    }
    scene.draws[6] = 0;
    scene.draws[2] = 0;
    scene.draws[3] = 9;
    scene.draws[18] = 1;
    scene.config[14] = 12;
    scene.sdf = vec![0; 20];
    scene.sdf[0] = 1;
    scene.sdf[3] = 16f32.to_bits();
    scene.sdf[4] = 16f32.to_bits();
    scene.sdf[12] = 1;
    scene.sdf[16] = 0xff336699;
    let mut record = [9, 0, 0, 0, 0, 0];
    let mut kind = 3;
    match mode {
        0 => {
            scene.draws[2] = u32::MAX;
            record = [2, 1, 0, 0, 0, 0xff336699];
            kind = 2;
        }
        1 => {
            scene.draws[2] = u32::MAX;
            scene.draws[20] = 0.125f32.to_bits();
            record = [1, 1, 0, 0, 0, 0];
            kind = 0;
        }
        2 => {
            scene.draws[2] = u32::MAX;
            scene.sdf[16] = 0;
            record = [1, 1, 0, 0, 0, 0];
            kind = 0;
        }
        3 => {
            record = [2, 0, 0, 0, 0, 0xff336699];
            kind = 2;
        }
        4 => scene.sdf[5] = 4f32.to_bits(),
        5 => scene.draws[19] = 2f32.to_bits(),
        6 => {
            scene.draws[4] = 0;
            kind = 0;
        }
        7 => {
            scene.sdf[0] = 5;
            scene.draws[3] = 1;
        }
        8 => {
            scene.sdf[0] = 14;
            scene.draws[3] = 1;
        }
        9 => {
            scene.sdf[0] = 2;
            kind = 0;
        }
        10 | 11 | 17 => {
            scene.sdf[12] = 7;
            scene.sdf[19] = if mode == 11 { 254 } else { 255 };
            if mode == 11 {
                kind = 0;
            } else {
                record = [13, 0, 0, 0, 0, 0];
            }
            if mode == 17 {
                scene.sdf[1] = 16f32.to_bits();
                scene.sdf[2] = 16f32.to_bits();
                scene.sdf[3] = 0;
                scene.sdf[4] = 0;
            }
        }
        12 => {
            scene.draws[6] = u32::MAX;
            kind = 0;
        }
        13 => {
            scene.sdf[16] = 0;
            kind = 0;
        }
        14..=16 => {
            scene.draws[1] = 0;
            scene.config[10] = 1;
            scene.config[11] = 3;
            scene.config[15] = 1;
            scene.text = vec![0, 3, 0, 0, 0, 0, 16, 0, 0, 4, 0, 0, 0, 8, 8, 0, 0];
            record = [10, 0, 0, 1, 3, 0];
            kind = 0;
        }
        18 => {
            scene.draws[2] = u32::MAX;
            scene.draws[4] = 0;
            kind = 0;
        }
        19 => {
            scene.draws[2] = u32::MAX;
            scene.draws[8] = 5;
            record = [11, 1, 0, 0, 0, 0];
            kind = 0;
        }
        20 => {
            scene.draws[2] = u32::MAX;
            scene.draws[6] = 2;
            scene.sdf[12] = 0;
            scene.sdf[14] = 1;
            scene.sdf[18] = 0xff336699;
            scene.backdrops[0] = u32::MAX;
            record = [2, u32::MAX, 0, 0, 0, 0xff336699];
            kind = 2;
        }
        _ => unreachable!(),
    }
    scene.work[..6].copy_from_slice(&[
        2,
        1,
        3,
        if (14..=16).contains(&mode) { 2 } else { 0 },
        1,
        if mode == 15 { 2 } else { 3 },
    ]);
    scene.expected = scene.work.clone();
    scene.expected[12..18].copy_from_slice(&record);
    scene.expected[18..24].fill(0);
    if mode == 14 || mode == 16 {
        let glyph_base = 6 + scene.config[7] as usize * 6;
        scene.expected[glyph_base + 1] = 0;
        if mode == 14 {
            scene.expected[glyph_base + 2] = 2;
        }
    }
    if bins {
        scene.expected[scene.kind_base] = kind;
    }
    scene
}

pub(super) fn glyph_page_scene(linked: bool, bins: bool) -> CountScene {
    let mut scene = count_scene(257, linked, 0);
    scene.reserve_streams(260, 520);
    let paint = paint_scene(14, false);
    scene.draws = paint.draws;
    scene.sdf = paint.sdf;
    scene.text = paint.text;
    scene.config[10] = 1;
    scene.config[11] = 3;
    scene.config[14] = 12;
    scene.config[15] = 1;
    let index_base = 6 + 260 * 6 + 520 + 2;
    for index in 0..257usize {
        let word = if linked {
            index / 256 * 257 + 1 + index % 256
        } else {
            index
        };
        scene.work[index_base + word] = 0;
    }
    scene.work[..6].copy_from_slice(&[258, 1, 259, 514, 1, 515]);
    scene.expected = scene.work.clone();
    for index in 0..257usize {
        let glyph = 1 + index * 2;
        scene.expected[12 + index * 6..18 + index * 6].copy_from_slice(&[
            10,
            0,
            0,
            glyph as u32,
            glyph as u32 + 2,
            0,
        ]);
        let base = 6 + 260 * 6 + glyph;
        scene.expected[base] = 0;
        scene.expected[base + 1] = 2;
    }
    scene.expected[12 + 257 * 6..18 + 257 * 6].fill(0);
    if bins {
        scene.expected[scene.kind_base] = 0;
    }
    scene
}

pub(super) fn emission_grid_scene(mode: u32) -> CountScene {
    let (mut scene, counts, _) = super::grid::grid_scene();
    let capacity = 3 + counts
        .iter()
        .map(|n| if *n == 0 { 0 } else { n + 1 })
        .sum::<u32>();
    scene.reserve_streams(capacity + 3, 0);
    for draw in scene.draws.chunks_exact_mut(31) {
        draw[6] = u32::MAX;
    }
    let selected: Vec<usize> = if mode == 1 {
        scene.config[18] = 1;
        scene.config[16] = 7;
        vec![321, 16, 289, 1, 17, 0, 322]
    } else {
        (0..counts.len()).collect()
    };
    let mut cursor = 3;
    let mut ranges = Vec::new();
    for (tile, count) in counts.iter().enumerate() {
        let len = if *count == 0 { 0 } else { count + 1 };
        scene.work[tile * 6..tile * 6 + 6].copy_from_slice(&[len, cursor, cursor + len, 0, 0, 0]);
        ranges.push(cursor);
        cursor += len;
    }
    scene.expected = scene.work.clone();
    let particle_base = counts.len() * 6;
    for tile in selected {
        let count = counts[tile];
        for index in 0..count {
            let base = particle_base + (ranges[tile] + index) as usize * 6;
            scene.expected[base..base + 6].copy_from_slice(&[9, 0, 0, 0, 0, 0]);
        }
        if count != 0 {
            let base = particle_base + (ranges[tile] + count) as usize * 6;
            scene.expected[base..base + 6].fill(0);
        }
        if mode == 2 {
            scene.expected[scene.kind_base + tile] = u32::from(count == 0);
        }
    }
    scene
}
