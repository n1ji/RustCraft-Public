use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon.rc");

    #[cfg(target_os = "windows")]
    embed_resource::compile("../../assets/icon.rc", embed_resource::NONE);

    copy_fidelityfx_runtime();
}

fn copy_fidelityfx_runtime() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let source =
        Path::new("../../third_party/FidelityFX-SDK/PrebuiltSignedDLL/amd_fidelityfx_vk.dll");
    println!("cargo:rerun-if-changed={}", source.display());
    if !source.is_file() {
        println!(
            "cargo:warning=FidelityFX runtime is unavailable; initialize the FidelityFX-SDK submodule"
        );
        return;
    }

    let out_dir =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR"));
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        println!("cargo:warning=unable to locate Cargo profile directory for FidelityFX runtime");
        return;
    };
    let destination = profile_dir.join("amd_fidelityfx_vk.dll");
    if let Err(error) = std::fs::copy(source, &destination) {
        panic!(
            "failed to copy {} to {}: {error}",
            source.display(),
            destination.display()
        );
    }
}
