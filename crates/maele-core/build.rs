//! Link the macOS frameworks that ONNX Runtime's prebuilt CoreML backend
//! references but does not declare. Only emitted for the `silero` feature,
//! which is the only thing that pulls ONNX Runtime in. Requires a macOS 14+
//! SDK (full Xcode); CommandLineTools 13.x cannot resolve MLComputePlan.

fn main() {
    #[cfg(target_os = "macos")]
    {
        if std::env::var_os("CARGO_FEATURE_SILERO").is_some() {
            println!("cargo:rustc-link-lib=framework=CoreML");
            println!("cargo:rustc-link-lib=framework=Foundation");
            println!("cargo:rustc-link-lib=framework=Accelerate");
        }
    }
}
