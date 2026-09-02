fn main() {
    // `tauri_build` embeds the Common Controls v6 manifest in the application
    // binary, but Rust's unit-test harness is a separate executable and does
    // not inherit that resource. Tauri's mock runtime links its dialog support,
    // which imports `TaskDialogIndirect`; without v6 the test process exits at
    // the loader before a single test runs. Under the explicit test feature we
    // replace, rather than duplicate, Tauri's manifest resource.
    #[cfg(windows)]
    if std::env::var_os("CARGO_FEATURE_TAURI_TEST").is_some() {
        let attributes = tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        tauri_build::try_build(attributes).expect("failed to run Tauri build script");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("windows-test-manifest.xml")
                .display()
        );
        return;
    }

    tauri_build::build();
}
