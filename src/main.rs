#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod ui;

/// 加载内嵌图标并按指定尺寸启动下载器窗口。 / Load the embedded icon and launch the downloader window at the configured size.
fn main() -> eframe::Result {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/app.png"))
        .expect("Embedded application icon is invalid");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(icon)
            .with_inner_size([900.0, 730.0])
            .with_min_inner_size([900.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "立创元器件下载器",
        options,
        Box::new(|context| Ok(Box::new(ui::DownloaderApp::new(context)))),
    )
}
