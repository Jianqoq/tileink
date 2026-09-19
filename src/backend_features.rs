// tileink owns one window renderer per build. Cargo unifies dependency features,
// so reject conflicts instead of silently selecting one of several backends.
#[cfg(not(any(feature = "wgpu", feature = "dx12", feature = "vulkan")))]
compile_error!("tileink requires exactly one backend feature: wgpu, dx12, or vulkan");

#[cfg(any(
    all(feature = "wgpu", feature = "dx12"),
    all(feature = "wgpu", feature = "vulkan"),
    all(feature = "dx12", feature = "vulkan")
))]
compile_error!(
    "tileink backend features are mutually exclusive; use --no-default-features --features dx12 (or vulkan) to replace the default wgpu backend"
);
