//! Embed the application mark in the native Windows executable.

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=assets/bonsai.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/bonsai.ico")
            .compile_for("bonsai-chat")?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
