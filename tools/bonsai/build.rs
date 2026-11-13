//! Embed the application icon before the launcher becomes a verified bundle.

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=../../src/chat-ui/assets/bonsai.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../src/chat-ui/assets/bonsai.ico")
            .compile_for("bonsai-launcher")?;
    }
    Ok(())
}
