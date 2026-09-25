#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
#[path = "build/native.rs"]
mod native_shaders;

#[path = "build/gpu_constants.rs"]
mod gpu_constants;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(tileink_native_runtime)");
    let platform = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if (platform == "windows" && cfg!(any(feature = "dx12", feature = "vulkan")))
        || (platform == "macos" && cfg!(feature = "metal"))
    {
        println!("cargo:rustc-cfg=tileink_native_runtime");
    }
    println!("cargo:rerun-if-changed=build/gpu_constants.rs");
    println!("cargo:rerun-if-changed=build/hlsl_source.rs");
    println!("cargo:rerun-if-changed=src/shaders/hlsl/constants.hlsli");
    gpu_constants::write_rust(&std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()))
        .expect("generate host GPU constants");
    #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
    native_shaders::generate()
        .unwrap_or_else(|error| panic!("native shader build failed: {error}"));
    println!("cargo:rerun-if-changed=build.rs");
}

#[path = "src/backend_features.rs"]
mod backend_features;
