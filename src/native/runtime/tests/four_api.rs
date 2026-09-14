use super::reference;
use crate::native::{
    NativeBackend,
    runtime::{Result, adapter::Adapter, compute::ComputeBatch},
};

pub(super) struct Routes {
    native: [Adapter; 2],
    reference: [reference::Reference; 2],
}
impl Routes {
    pub(super) fn new() -> Result<Self> {
        Self::with_features(wgpu::Features::empty())
    }
    pub(super) fn with_features(features: wgpu::Features) -> Result<Self> {
        let identity = std::env::var("TILEINK_NATIVE_GPU")?;
        // Enable native debug validation before creating the corresponding wgpu devices.
        let native = [
            Adapter::new(NativeBackend::Dx12, &identity)?,
            Adapter::new(NativeBackend::Vulkan, &identity)?,
        ];
        let reference = [
            reference::Reference::with_features(wgpu::Backends::DX12, &identity, features)?,
            reference::Reference::with_features(wgpu::Backends::VULKAN, &identity, features)?,
        ];
        Ok(Self { native, reference })
    }
    pub(super) fn reference_output(&self, batch: &ComputeBatch) -> Result<Vec<Vec<u8>>> {
        self.reference[0].execute_compute(batch)
    }
    pub(super) fn check(
        &self,
        batch: &ComputeBatch,
        expected: &[Vec<u8>],
        case: &str,
    ) -> Result<()> {
        self.check_variant(batch, expected, case, None)
    }
    pub(super) fn check_variant(
        &self,
        batch: &ComputeBatch,
        expected: &[Vec<u8>],
        case: &str,
        variant: Option<reference::FilterVariant>,
    ) -> Result<()> {
        let mut mismatches = Vec::new();
        for (route, result) in self
            .reference
            .iter()
            .map(|r| r.execute_variant(batch, variant))
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
                if a.len() != b.len() || first.is_some() {
                    mismatches.push(format!("{case} route {route} buffer {buffer} first mismatch {first:?}, actual/expected {:?}",first.map(|i|(a[i],b[i]))));
                }
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        Ok(())
    }
    pub(super) fn validate(&self) -> Result<()> {
        for adapter in &self.native {
            adapter.assert_valid()?;
        }
        Ok(())
    }
}
