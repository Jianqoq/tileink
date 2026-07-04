fn text_glyph_base(glyph_ix: u32) -> u32 {
    return config.text_run_count * GLYPH_RUN_RECORD_WORDS + glyph_ix * GLYPH_RECORD_WORDS;
}

fn text_image_base(image_ix: u32) -> u32 {
    return config.text_run_count * GLYPH_RUN_RECORD_WORDS +
        config.text_glyph_count * GLYPH_RECORD_WORDS +
        image_ix * GLYPH_IMAGE_RECORD_WORDS;
}

fn text_run_at(run_ix: u32) -> GlyphRunRecord {
    let base = run_ix * GLYPH_RUN_RECORD_WORDS;
    return GlyphRunRecord(text_blob[base], text_blob[base + 1u]);
}

fn glyph_at(glyph_ix: u32) -> GlyphRecord {
    let base = text_glyph_base(glyph_ix);
    return GlyphRecord(text_blob[base], bitcast<i32>(text_blob[base + 1u]), bitcast<i32>(text_blob[base + 2u]));
}

fn glyph_image_at(image_ix: u32) -> GlyphImageRecord {
    let base = text_image_base(image_ix);
    return GlyphImageRecord(
        bitcast<i32>(text_blob[base]),
        bitcast<i32>(text_blob[base + 1u]),
        text_blob[base + 2u],
        text_blob[base + 3u],
        text_blob[base + 4u],
        text_blob[base + 5u],
    );
}
