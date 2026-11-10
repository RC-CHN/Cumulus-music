fn main() {
    const NATIVE_OPEN_NULL_PROOF: &str = "CUMULUS_BUILD_WINDOWS_NATIVE_OPEN_NULL_PROOF";
    println!("cargo:rerun-if-env-changed={NATIVE_OPEN_NULL_PROOF}");
    println!("cargo:rustc-check-cfg=cfg(cumulus_windows_native_open_null_proof)");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
        && std::env::var(NATIVE_OPEN_NULL_PROOF).as_deref() == Ok("1")
    {
        println!("cargo:rustc-cfg=cumulus_windows_native_open_null_proof");
    }
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows_msvc {
        // Tauri's resource manifest otherwise reaches the app binary but not
        // integration tests. Apply its Common Controls v6 dependency to every
        // executable, including WebView probes. See Tauri's api example build.rs.
        tauri_build::try_build(
            tauri_build::Attributes::new()
                .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
        )
        .expect("Tauri build failed");
        let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    } else {
        tauri_build::build();
    }
}
