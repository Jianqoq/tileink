//! Exercise the production diagnostic callback without a Vulkan loader or GPU.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "Test setup and assertions abort on failure; production builds are checked separately"
    )
)]
#[path = "../src/native/runtime/vulkan/validation.rs"]
mod validation;
use ash::vk;
use std::{
    ffi::CStr,
    sync::{Arc, Mutex},
};

fn notify(
    messages: &Mutex<Vec<String>>,
    kind: vk::DebugUtilsMessageTypeFlagsEXT,
    message: Option<&CStr>,
) {
    let data = vk::DebugUtilsMessengerCallbackDataEXT {
        p_message: message.map_or(std::ptr::null(), CStr::as_ptr),
        ..Default::default()
    };
    // SAFETY: The callback data, optional C string, and message storage remain live throughout this synchronous call.
    let result = unsafe {
        validation::callback(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
            kind,
            &raw const data,
            std::ptr::from_ref(messages).cast_mut().cast(),
        )
    };
    assert_eq!(result, vk::FALSE);
}

#[test]
fn callback_preserves_utf8_empty_messages_and_order_without_aborting() {
    let messages = Mutex::new(vec![]);
    for text in [c"first", c"GPU 错误", c""] {
        notify(
            &messages,
            vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION,
            Some(text),
        );
    }
    assert_eq!(*messages.lock().unwrap(), ["first", "GPU 错误", ""]);
}

#[test]
fn absent_device_address_message_preserves_collected_diagnostics() {
    let messages = Mutex::new(vec!["prior".into()]);
    notify(
        &messages,
        vk::DebugUtilsMessageTypeFlagsEXT::DEVICE_ADDRESS_BINDING,
        None,
    );
    assert_eq!(*messages.lock().unwrap(), ["prior"]);
}

#[test]
#[allow(
    clippy::significant_drop_tightening,
    reason = "The mutex guard must remain held during the deliberate panic to test poisoning"
)]
fn poisoned_collection_does_not_unwind_through_the_native_callback() {
    let messages = Mutex::new(vec![]);
    assert!(
        std::panic::catch_unwind(|| {
            let mut guard = messages.lock().unwrap();
            guard.push("before".into());
            panic!("deliberate diagnostic collection poisoning");
        })
        .is_err()
    );
    notify(
        &messages,
        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL,
        Some(c"after"),
    );
    assert_eq!(
        *messages.lock().unwrap_err().into_inner(),
        ["before", "after"]
    );
}

#[test]
fn independent_context_owners_receive_only_their_own_messages() {
    let first = Arc::new(Mutex::new(vec![]));
    let second = Arc::new(Mutex::new(vec![]));
    notify(
        &first,
        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL,
        Some(c"first"),
    );
    notify(
        &second,
        vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        Some(c"second"),
    );
    assert_eq!(*first.lock().unwrap(), ["first"]);
    assert_eq!(*second.lock().unwrap(), ["second"]);
}

#[test]
fn registration_preserves_callback_owner_filters_and_native_return_contract() {
    let messages = Arc::new(Mutex::new(vec![]));
    let info = validation::info(&messages);
    assert_eq!(info.p_user_data, Arc::as_ptr(&messages).cast_mut().cast());
    assert_eq!(
        info.message_severity,
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
            | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
    );
    assert_eq!(
        info.message_type,
        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
            | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
            | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
    );
    let data = vk::DebugUtilsMessengerCallbackDataEXT::default().message(c"registered");
    assert_eq!(
        // SAFETY: The registered Arc and callback data remain live throughout this synchronous invocation.
        unsafe {
            info.pfn_user_callback.unwrap()(
                vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL,
                &raw const data,
                info.p_user_data,
            )
        },
        vk::FALSE
    );
    assert_eq!(*messages.lock().unwrap(), ["registered"]);
}
