use super::*;
use crate::Canvas;
/// A consumed per-use synchronization contract. It is intentionally not Clone/Copy.
pub struct NativeTargetUse<'a> {
    pub(crate) target: NativeRenderTarget<'a>,
    pub(crate) synchronization: interop::Synchronization,
}
/// The outgoing state promised by an accepted target submission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeTargetState {
    #[cfg(all(target_os = "windows", feature = "dx12"))]
    Dx12(windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES),
    #[cfg(all(target_os = "windows", feature = "vulkan"))]
    Vulkan(interop::vulkan::ImageState),
}
pub struct NativeTargetSubmission {
    pub submission: NativeSubmission,
    pub outgoing: NativeTargetState,
}
impl NativeRenderer {
    pub fn render_to_target_use(
        &mut self,
        canvas: &Canvas,
        usage: NativeTargetUse<'_>,
    ) -> Result<NativeTargetSubmission, NativeError> {
        let outgoing = usage.synchronization.outgoing();
        let submission = self.submit_synchronized(
            crate::render::retained::SelectedScene::Borrowed(canvas),
            None,
            false,
            Some(usage.target),
            Some(usage.synchronization),
        )?;
        Ok(NativeTargetSubmission {
            submission,
            outgoing,
        })
    }
    pub fn render_retained_to_target_use(
        &mut self,
        scene: &crate::RetainedScene,
        usage: NativeTargetUse<'_>,
    ) -> Result<NativeTargetSubmission, NativeError> {
        let outgoing = usage.synchronization.outgoing();
        let submission = self.submit_retained_synchronized(
            scene,
            None,
            false,
            Some(usage.target),
            Some(usage.synchronization),
        )?;
        Ok(NativeTargetSubmission {
            submission,
            outgoing,
        })
    }
}

impl NativeRenderer {
    pub fn render_with_text_to_target_use(
        &mut self,
        canvas: &Canvas,
        fonts: &mut crate::TextFontSystem,
        text: &mut crate::TextContext,
        usage: NativeTargetUse<'_>,
    ) -> Result<NativeTargetSubmission, NativeError> {
        let outgoing = usage.synchronization.outgoing();
        let submission = self.submit_synchronized(
            crate::render::retained::SelectedScene::Borrowed(canvas),
            Some((fonts, text)),
            false,
            Some(usage.target),
            Some(usage.synchronization),
        )?;
        Ok(NativeTargetSubmission {
            submission,
            outgoing,
        })
    }
    pub fn render_retained_with_text_to_target_use(
        &mut self,
        scene: &crate::RetainedScene,
        fonts: &mut crate::TextFontSystem,
        text: &mut crate::TextContext,
        usage: NativeTargetUse<'_>,
    ) -> Result<NativeTargetSubmission, NativeError> {
        let outgoing = usage.synchronization.outgoing();
        let submission = self.submit_retained_synchronized(
            scene,
            Some((fonts, text)),
            false,
            Some(usage.target),
            Some(usage.synchronization),
        )?;
        Ok(NativeTargetSubmission {
            submission,
            outgoing,
        })
    }
}
