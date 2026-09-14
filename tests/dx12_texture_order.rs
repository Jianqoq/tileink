#![cfg(windows)]

#[path = "support/dx12_texture_order.rs"]
mod support;

#[test]
#[ignore = "requires a DX12 hardware GPU and TILEINK_PARITY_DXCOMPILER"]
fn write_only_texture_dispatches_preserve_submission_order() {
    let writes = support::TextureWrites::new();
    // Reuse the resource across submissions; automatic first-use initialization
    // can hide the missing same-state barrier on the very first frame.
    for frame in 0..16 {
        for pairs in [1, 8] {
            // Readback changes the texture to COPY_SOURCE. Keep it in UAV state
            // across normal submissions before checking: readback after every
            // submission can accidentally mask this missing dependency.
            for _ in 0..64 {
                let encoder = writes.encode(pairs);
                writes.queue.submit([encoder.finish()]);
                writes
                    .device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
            }
            writes.assert_cleared(pairs, frame);
        }
    }
}
