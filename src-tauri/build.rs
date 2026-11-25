fn main() {
    tauri_build::build();

    // Delay-load comctl32.dll on Windows so `--lib` test binaries (which do not receive
    // tauri-build's `rustc-link-arg-bins` Common Controls v6 manifest) can start cleanly.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
        println!("cargo:rustc-link-lib=delayimp");
    }
}
