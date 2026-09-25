use super::*;
use crate::Canvas;
/// A consumed per-use synchronization contract. It is intentionally not Clone/Copy.
pub struct NativeTargetUse<'a> {
    pub(crate) target: NativeRenderTarget<'a>,
    pub(crate) synchronization: interop::Synchronization,
}
/// The outgoing state promised by an accepted target submission.
/// Backend features are exclusive, so the state has a concrete layout without a backend tag.
#[cfg_attr(target_os = "windows", repr(transparent))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeTargetState {
    #[cfg(all(target_os = "windows", feature = "dx12"))]
    pub state: windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES,
    #[cfg(all(target_os = "windows", feature = "vulkan"))]
    pub state: interop::vulkan::ImageState,
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::NativeTargetState;

    #[test]
    fn outgoing_state_has_concrete_backend_layout() {
        #[cfg(feature = "dx12")]
        type State = windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES;
        #[cfg(feature = "vulkan")]
        type State = super::interop::vulkan::ImageState;
        assert_eq!(size_of::<NativeTargetState>(), size_of::<State>());
        assert_eq!(align_of::<NativeTargetState>(), align_of::<State>());
    }
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
