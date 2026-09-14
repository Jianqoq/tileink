use super::{Result, cases};
use crate::native::{NativeBackend, runtime::adapter::Adapter};
use crate::render::commands::CommandBatch;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn shared_batches_submit_once_stage_uniforms_and_preserve_prefixes() -> Result<()> {
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        let adapter = Adapter::new(backend, &identity)?;
        let fixtures = cases::cases();
        let mut batch = CommandBatch::from_adapter(adapter.clone(), "M3 multi-dispatch");
        for case in &fixtures {
            batch.try_encoder().unwrap().dispatch(case.dispatch.clone());
        }
        let receipt = batch.try_finish().unwrap();
        assert_eq!(receipt.submissions, 1);
        assert_eq!(adapter.pending_count(), 1);
        let bytes = adapter.readback(&receipt.last_submission.unwrap())?;
        assert_eq!(
            bytes,
            fixtures
                .iter()
                .map(|c| c.expected.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(adapter.pending_count(), 0);

        // Slot rollover submits the prefix before offset zero is reused.
        let buffer = adapter.uniform_buffer();
        let mut batch = CommandBatch::from_adapter(adapter.clone(), "M3 uniforms");
        let fixture = &fixtures[1];
        for value in [0x12345678u32, 0x87654321, 0x11223344] {
            let mut params = fixture.params;
            params.value[0] = value;
            let offset = batch
                .try_write_uniform_slot(&buffer, 32, 256, 2, bytemuck::bytes_of(&params))
                .unwrap();
            batch.try_encoder().unwrap().dispatch_uniform(
                fixture.dispatch.clone(),
                buffer.clone(),
                offset,
            );
        }
        let prefix = batch.abort();
        assert_eq!(prefix.submissions, 1);
        let outputs = adapter.readback(&prefix.last_submission.unwrap())?;
        assert_eq!(outputs.len(), 2);
        for (output, value) in outputs.iter().zip([0x12345678u32, 0x87654321]) {
            let mut expected = fixture.expected.clone();
            expected[8..12].copy_from_slice(&value.to_le_bytes());
            assert_eq!(*output, expected);
        }
        assert_eq!(adapter.pending_count(), 0);
        adapter.assert_valid()?;
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn rejected_work_preserves_device_and_receipts_pin_the_context() -> Result<()> {
    use crate::render::{
        backend::{BatchAdapter, SubmitError},
        upload::uniforms::UniformWrites,
    };
    let identity = std::env::var("TILEINK_NATIVE_GPU")?;
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        let mut adapter = Adapter::new(backend, &identity)?;
        let mut other = Adapter::new(backend, &identity)?;
        let fixtures = cases::cases();
        let mut foreign = other.create_encoder("foreign").unwrap();
        foreign.dispatch(fixtures[1].dispatch.clone());
        assert!(matches!(
            adapter.submit(foreign, &UniformWrites::default()),
            Err(SubmitError::Rejected(_))
        ));
        let mut encoder = adapter.create_encoder("bad bounds").unwrap();
        let mut bad = fixtures[1].dispatch.clone();
        bad.params.count = u32::MAX;
        encoder.dispatch(bad);
        assert!(matches!(
            adapter.submit(encoder, &UniformWrites::default()),
            Err(SubmitError::Rejected(_))
        ));
        for offset in [1u64, 256, u64::MAX] {
            let buffer = adapter.uniform_buffer();
            let mut uniforms = UniformWrites::default();
            uniforms.write(&buffer, 32, 256, 1, bytemuck::bytes_of(&fixtures[1].params));
            let mut encoder = adapter.create_encoder("bad uniform range").unwrap();
            encoder.dispatch_uniform(fixtures[1].dispatch.clone(), buffer, offset);
            assert!(matches!(
                adapter.submit(encoder, &uniforms),
                Err(SubmitError::Rejected(_))
            ));
        }
        let buffer = other.uniform_buffer();
        let mut uniforms = UniformWrites::default();
        uniforms.write(&buffer, 32, 256, 1, bytemuck::bytes_of(&fixtures[1].params));
        let mut encoder = adapter.create_encoder("foreign uniform").unwrap();
        encoder.dispatch_uniform(fixtures[1].dispatch.clone(), buffer, 0);
        assert!(matches!(
            adapter.submit(encoder, &uniforms),
            Err(SubmitError::Rejected(_))
        ));
        assert_eq!(adapter.pending_count(), 0);
        let mut batch = CommandBatch::from_adapter(adapter.clone(), "receipt owner");
        batch
            .try_encoder()
            .unwrap()
            .dispatch(fixtures[1].dispatch.clone());
        let receipt = batch.try_finish().unwrap().last_submission.unwrap();
        assert!(other.readback(&receipt).is_err());
        drop(adapter);
        let recreated = Adapter::new(backend, &identity)?;
        assert!(recreated.readback(&receipt).is_err());
        assert_eq!(receipt.readback()?, vec![fixtures[1].expected.clone()]);
        assert!(receipt.readback().is_err());
        receipt.assert_valid()?;
        other.assert_valid()?;
        recreated.assert_valid()?;
    }
    Ok(())
}
