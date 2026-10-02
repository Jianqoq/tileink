use super::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn immutable_samplers_are_shared_across_in_flight_frames() -> Result<()> {
    let mut device = Vulkan::new(&std::env::var("TILEINK_NATIVE_GPU")?)?;
    let messages = device.validation_messages();
    let mut batch = super::super::compute::ComputeBatch::new();
    use crate::native::runtime::compute::SamplerFilter;
    batch.sampler(SamplerFilter::Nearest)?;
    batch.sampler(SamplerFilter::Linear)?;
    batch.sampler(SamplerFilter::Nearest)?;
    let a = device.submit_compute(&batch)?;
    let b = device.submit_compute(&batch)?;
    let identities = |ticket: &super::super::submissions::Ticket| {
        let work::Work::Compute(frame) = device.pending.get(ticket).unwrap() else {
            unreachable!()
        };
        frame.sampler_identities()
    };
    let first = identities(&a);
    assert_eq!(
        first[0], first[2],
        "immutable sampler duplicates must share ownership"
    );
    assert_ne!(first[0], first[1], "nearest and linear remain distinct");
    assert_eq!(first, identities(&b));
    device.readback_batch(&b)?;
    device.readback_batch(&a)?;
    let c = device.submit_compute(&batch)?;
    let work::Work::Compute(frame) = device.pending.get(&c)? else {
        unreachable!()
    };
    assert_eq!(first, frame.sampler_identities());
    device.readback_batch(&c)?;
    assert!(messages.lock().unwrap().is_empty());
    Ok(())
}
