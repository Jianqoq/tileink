#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
#[path = "build/hex.rs"]
mod hex;
#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
#[path = "build/native.rs"]
mod native_shaders;

#[path = "build/gpu_constants.rs"]
mod gpu_constants;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rustc-check-cfg=cfg(tileink_native_runtime)");
    let platform = std::env::var("CARGO_CFG_TARGET_OS")?;
    if (platform == "windows" && cfg!(any(feature = "dx12", feature = "vulkan")))
        || (platform == "macos" && cfg!(feature = "metal"))
    {
        println!("cargo:rustc-cfg=tileink_native_runtime");
    }
    println!("cargo:rerun-if-changed=build/gpu_constants.rs");
    println!("cargo:rerun-if-changed=build/hlsl_source.rs");
    println!("cargo:rerun-if-changed=src/shaders/hlsl/constants.hlsli");
    gpu_constants::write_rust(&std::path::PathBuf::from(
        std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?,
    ))?;
    #[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
    native_shaders::generate()?;
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}

#[path = "src/backend_features.rs"]
mod backend_features;
