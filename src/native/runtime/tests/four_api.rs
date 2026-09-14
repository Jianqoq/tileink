use super::reference;
use crate::native::{
    NativeBackend,
    runtime::{Result, adapter::Adapter, compute::ComputeBatch},
};

pub(super) struct Routes {
    used_canvas_reference: std::cell::Cell<bool>,
    native: [Adapter; 2],
    reference: [reference::Reference; 2],
}
impl Routes {
    pub(super) fn canvas_reference(&self, canvas: &crate::Canvas) -> Result<Vec<Vec<u8>>> {
        self.used_canvas_reference.set(true);
        let expected = self.reference[0].render_canvas(canvas)?;
        assert_eq!(
            self.reference[1].render_canvas(canvas)?,
            expected,
            "production wgpu DX12/Vulkan Canvas pixels"
        );
        Ok(vec![expected])
    }

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
        Ok(Self {
            native,
            reference,
            used_canvas_reference: Default::default(),
        })
    }
    pub(super) fn filter_reference_output(
        &self,
        batch: &ComputeBatch,
        variant: reference::FilterVariant,
    ) -> Result<Vec<Vec<u8>>> {
        self.reference[0].execute_variant(batch, Some(variant))
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
        self.check_selected(batch, expected, case, variant, None)
    }
    pub(super) fn fine_reference(
        &self,
        batch: &ComputeBatch,
        variant: reference::FineVariant,
    ) -> Result<Vec<Vec<u8>>> {
        self.reference[0].execute_fine_variant(batch, variant)
    }

    pub(super) fn check_fine(
        &self,
        batch: &ComputeBatch,
        expected: &[Vec<u8>],
        case: &str,
        variant: reference::FineVariant,
    ) -> Result<()> {
        self.check_selected(batch, expected, case, None, Some(variant))
    }
    pub(super) fn render_reference(
        &self,
        batch: &ComputeBatch,
        filter: reference::FilterVariant,
        fine: reference::FineVariant,
    ) -> Result<Vec<Vec<u8>>> {
        self.reference[0].execute_selected(batch, Some(filter), Some(fine))
    }

    pub(super) fn check_render(
        &self,
        batch: &ComputeBatch,
        expected: &[Vec<u8>],
        case: &str,
        filter: reference::FilterVariant,
        fine: reference::FineVariant,
    ) -> Result<()> {
        self.check_selected(batch, expected, case, Some(filter), Some(fine))
    }

    fn check_selected(
        &self,
        batch: &ComputeBatch,
        expected: &[Vec<u8>],
        case: &str,
        variant: Option<reference::FilterVariant>,
        fine: Option<reference::FineVariant>,
    ) -> Result<()> {
        let mut mismatches = Vec::new();
        for (route, result) in self
            .reference
            .iter()
            .map(|r| r.execute_selected(batch, variant, fine))
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
                    mismatches.push(format!("{case} route {route} buffer {buffer} first mismatch {first:?}, actual/expected {:?}",first.map(|i|(&a[i/4*4..(i/4*4+4).min(a.len())],&b[i/4*4..(i/4*4+4).min(b.len())]))));
                }
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        Ok(())
    }
    pub(super) fn assert_reference_pipeline_builds(&self, expected: usize) {
        for reference in &self.reference {
            assert_eq!(
                reference.compute_pipeline_builds(),
                expected,
                "reference variants compile exactly once per device"
            );
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        for adapter in &self.native {
            if self.used_canvas_reference.get() {
                adapter.assert_valid_with_wgpu_clears()?;
            } else {
                adapter.assert_valid()?;
            }
        }
        Ok(())
    }
}
