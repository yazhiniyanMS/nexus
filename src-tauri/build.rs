fn main() {
    let msvc =
        std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !msvc {
        tauri_build::build();
        return;
    }
    // Tauri needs the Common Controls v6 manifest. By default tauri-build embeds it as a resource
    // in the application only, so test executables load the old comctl32 and crash with
    // STATUS_ENTRYPOINT_NOT_FOUND. Embedding it through the linker instead puts it into every
    // executable this crate builds: the app and its tests.
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("failed to run tauri-build");
    let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    println!("cargo:rustc-link-arg=/WX");
}
