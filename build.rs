/// 为 Windows 构建嵌入程序图标并登记资源变更依赖。 / Embed the Windows executable icon and track resource changes.
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
