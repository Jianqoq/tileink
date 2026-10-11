use ash::vk;
use std::sync::Mutex;

/// Both instance-creation and persistent messengers borrow the same live owner.
pub(super) fn info(messages: &Mutex<Vec<String>>) -> vk::DebugUtilsMessengerCreateInfoEXT<'_> {
    vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(callback))
        .user_data(std::ptr::from_ref(messages).cast_mut().cast())
}

pub(super) unsafe extern "system" fn callback(
    _severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _kind: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    // Vulkan supplies valid callback data and the user pointer registered by
    // info(), while its context still owns the Arc. Only message text is optional.
    unsafe {
        if !(*data).p_message.is_null() {
            let messages = &*user.cast::<Mutex<Vec<String>>>();
            let text = std::ffi::CStr::from_ptr((*data).p_message)
                .to_string_lossy()
                .into_owned();
            // The FFI callback cannot propagate a poisoned-lock panic. Vec stays
            // valid after unwinding; retain the native diagnostic for assert_valid.
            messages
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(text);
        }
    }
    vk::FALSE
}
