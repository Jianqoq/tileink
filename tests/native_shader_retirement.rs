#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "Test setup and assertions abort on failure; production builds are checked separately"
    )
)]
#[path = "../src/native/runtime/dx12/retirement.rs"]
mod retirement;
