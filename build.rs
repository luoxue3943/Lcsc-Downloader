/// 构建时将应用图标嵌入 Windows 可执行文件。 / Embed the application icon into the Windows executable during compilation.
fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/app.ico")
            .compile()
            .expect("Failed to embed the application icon");
    }
}
