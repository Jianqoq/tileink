// tileink owns one window renderer per build. Cargo unifies dependency features,
// so reject conflicts instead of silently selecting one of several backends.
#[cfg(not(any(
    feature = "wgpu",
    feature = "dx12",
    feature = "vulkan",
    feature = "metal"
)))]
compile_error!("tileink requires exactly one backend feature: wgpu, dx12, vulkan, or metal");

#[cfg(any(
    all(feature = "wgpu", feature = "dx12"),
    all(feature = "wgpu", feature = "vulkan"),
    all(feature = "dx12", feature = "vulkan"),
    all(
        feature = "metal",
        any(feature = "wgpu", feature = "dx12", feature = "vulkan")
    )
))]
compile_error!(
    "tileink backend features are mutually exclusive; use --no-default-features --features dx12 (or vulkan, metal) to replace the default wgpu backend"
);
