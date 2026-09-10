fn main() {
    println!("cargo:rerun-if-changed=assets/portweave.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/portweave.ico")
            .compile()
            .expect("failed to embed the PortWeave Windows icon");
    }
}
