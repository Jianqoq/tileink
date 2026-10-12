//! Completion is fallible; copying completed shared readback bytes is not.
use super::*;
use crate::native::runtime::gpu_support;

#[test]
#[ignore = "requires TILEINK_NATIVE_GPU and MTL_DEBUG_LAYER=1"]
fn completed_frame_materializes_mixed_readbacks_in_request_order() -> Result<()> {
    let metal = Metal::with_options(&gpu_support::options()?)?;
    for (width, height, layers) in [(1, 1, 1), (17, 3, 2), (64, 2, 3), (65, 2, 1)] {
        let pixels: Vec<_> = (0..width * height * layers * 4)
            .map(|i| (i * 19 + 7) as u8)
            .collect();
        let words = vec![0x12, 0x34, 0x56, 0x78];
        let mut batch = ComputeBatch::new();
        let buffer = batch.buffer(words.clone())?;
        let image = batch.texture_array_rgba8([width, height, layers], pixels.clone())?;
        // Output order differs from allocation order and spans padded rows and
        // array layers. Repeat reads must not mutate or consume shared storage.
        batch.readback(image)?;
        batch.readback(buffer)?;
        let frame = Frame::record(&metal, &batch)?;
        drop(batch);
        frame.command.commit();
        frame.wait()?;
        let image_bytes: Vec<u8> = frame.outputs[0].read();
        assert_eq!(image_bytes, pixels);
        let outputs: Vec<Vec<u8>> = frame.readback();
        assert_eq!(outputs, vec![pixels, words]);
    }
    metal.assert_valid()
}
