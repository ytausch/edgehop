fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=assets/edgehop.ico");

    // Embed the icon that Explorer and the taskbar show for edgehop.exe.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/edgehop.ico")
            .compile()
            .expect("failed to embed the Windows icon");
    }
}
