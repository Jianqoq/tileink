use ash::vk;

pub(super) unsafe extern "system" fn callback(
    _severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _kind: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    // Vulkan invokes this only while the context owns the Arc backing user.
    unsafe {
        if !data.is_null() && !user.is_null() && !(*data).p_message.is_null() {
            let messages = &*user.cast::<std::sync::Mutex<Vec<String>>>();
            let text = std::ffi::CStr::from_ptr((*data).p_message)
                .to_string_lossy()
                .into_owned();
            messages
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(text);
        }
    }
    vk::FALSE
}
