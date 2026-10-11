use super::filter_config::FilterConfig;

#[test]
fn filter_defaults_preserve_the_native_uniform_bytes() {
    assert_eq!(std::mem::size_of::<FilterConfig>(), 512);
    assert_eq!(std::mem::align_of::<FilterConfig>(), 4);
    assert_eq!(std::mem::offset_of!(FilterConfig, rounding_zero), 48);
    assert_eq!(std::mem::offset_of!(FilterConfig, downsample), 52);
    assert_eq!(std::mem::offset_of!(FilterConfig, offset_x), 128);
    assert_eq!(std::mem::offset_of!(FilterConfig, matrix_r), 432);
    assert_eq!(std::mem::offset_of!(FilterConfig, matrix_bias), 496);
    // Preserve positive floating zero, signed zero, arrays and explicit ABI
    // padding alike; only the unit downsample factor is nonzero.
    let config = FilterConfig::default();
    let words: &[u32] = bytemuck::cast_slice(std::slice::from_ref(&config));
    for (index, &word) in words.iter().enumerate() {
        assert_eq!(word, u32::from(index == 13), "uniform word {index}");
    }
}
