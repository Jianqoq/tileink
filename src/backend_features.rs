// Each build owns at most one native renderer. Cargo unifies dependency features,
// so reject conflicts instead of silently choosing a backend.
#[cfg(any(
    all(feature = "dx12", feature = "vulkan"),
    all(feature = "dx12", feature = "metal"),
    all(feature = "vulkan", feature = "metal"),
))]
compile_error!(
    "tileink backend features are mutually exclusive; select one of dx12, vulkan, or metal"
);
