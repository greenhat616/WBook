fn main() {
    // The icons are compiled into the executable and the window icon, but
    // tauri-build only watches tauri.conf.json, so a new icon would otherwise
    // never reach a build that is not clean.
    println!("cargo:rerun-if-changed=icons");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Tauri links its Common Controls manifest only into application binaries.
        // Let the linker embed it for examples and tests too, without duplicating resources.
        tauri_build::try_build(
            tauri_build::Attributes::new()
                .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
        )
        .expect("failed to build Tauri resources");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    } else {
        tauri_build::build();
    }
}
