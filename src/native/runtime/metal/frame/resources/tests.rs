use super::*;

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn uploads_after_host_resources_preserve_array_layers_and_readback_lifetimes() -> Result<()> {
    let metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    let mut batch = ComputeBatch::new();
    let host = [3u32, 7, 11, 173];
    let host_buffer = batch.buffer(bytemuck::cast_slice(&host).to_vec())?;
    batch.sampler(SamplerFilter::Linear);
    let pixels: Vec<_> = (0..17 * 3 * 2 * 4).map(|i| (i % 251) as u8).collect();
    let array = batch.texture_array_rgba8([17, 3, 2], pixels.clone())?;
    let image = batch.texture_rgba8([1, 1], vec![13, 17, 19, 255])?;
    batch.texture_table(&[image])?;
    let last = batch.buffer(vec![23; 4])?;
    for id in [host_buffer, array, image, last] {
        batch.readback(id)?;
    }
    let frame = Frame::record(&metal, &batch)?;
    drop(batch);
    frame.command.commit();
    frame.wait()?;
    assert_eq!(
        frame.readback()?,
        vec![
            bytemuck::cast_slice(&host).to_vec(),
            pixels,
            vec![13, 17, 19, 255],
            vec![23; 4],
        ]
    );
    metal.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn failed_allocation_closes_an_upload_encoder_before_command_buffer_reuse() -> Result<()> {
    let metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    let mut batch = ComputeBatch::new();
    batch.texture_rgba8([1, 1], vec![3, 7, 11, 255])?;
    // Shared dimensions are valid, but this exceeds Metal's texture limit.
    // The preceding texture upload has already opened the blit encoder.
    batch.texture_rgba8([16385, 1], vec![0; 16385 * 4])?;
    let command = metal.queue.commandBuffer().ok_or("Metal command buffer")?;
    let mut staging = Vec::new();
    let error = allocate(&metal.device, &command, &batch, &mut staging)
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "Metal texture dimensions exceed supported limits"
    );
    let encoder = command.computeCommandEncoder().ok_or("next encoder")?;
    encoder.endEncoding();
    let completion = crate::native::runtime::metal::completion::Completion::new(&command);
    drop(batch);
    command.commit();
    completion.wait(&command, std::time::Duration::from_secs(30))?;
    metal.assert_valid()
}
