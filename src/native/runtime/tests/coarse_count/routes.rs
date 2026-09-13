use super::*;

pub(super) struct Routes {
    native: [Adapter; 2],
    reference: [reference::Reference; 2],
}
impl Routes {
    pub(super) fn new() -> Result<Self> {
        let identity = std::env::var("TILEINK_NATIVE_GPU")?;
        // Enable native debug validation before creating the corresponding wgpu devices.
        let native = [
            Adapter::new(NativeBackend::Dx12, &identity)?,
            Adapter::new(NativeBackend::Vulkan, &identity)?,
        ];
        let reference = [
            reference::Reference::new(wgpu::Backends::DX12, &identity)?,
            reference::Reference::new(wgpu::Backends::VULKAN, &identity)?,
        ];
        Ok(Self { native, reference })
    }
    pub(super) fn check(
        &self,
        batch: &ComputeBatch,
        expected: &[Vec<u8>],
        case: &str,
    ) -> Result<()> {
        for (route, result) in self
            .reference
            .iter()
            .map(|r| r.execute_compute(batch))
            .chain(self.native.iter().map(|r| {
                r.submit_compute(batch)
                    .map_err(|e| format!("{e:?}").into())
                    .and_then(|s| s.readback())
            }))
            .enumerate()
        {
            let actual = result?;
            assert_eq!(
                actual.len(),
                expected.len(),
                "{case} route {route} readbacks"
            );
            for (buffer, (a, b)) in actual.iter().zip(expected).enumerate() {
                let first = a.iter().zip(b).position(|(a, b)| a != b);
                assert!(
                    a.len() == b.len() && first.is_none(),
                    "{case} route {route} buffer {buffer} first mismatch {first:?}"
                );
            }
        }
        Ok(())
    }
    pub(super) fn validate(&self) -> Result<()> {
        for adapter in &self.native {
            adapter.assert_valid()?;
        }
        Ok(())
    }
}
