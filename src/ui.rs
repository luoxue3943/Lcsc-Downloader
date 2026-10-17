use std::{
    fs,
    path::Path,
    sync::mpsc::{self, Receiver},
    thread,
};

use eframe::egui::{self, Color32, RichText};
use lcsc_downloader::{
    config::{self, Settings},
    download::{self, DownloadEvent, DownloadRequest},
    input::{decode_text, parse_ids},
};
use tokio_util::sync::CancellationToken;

const BLUE: Color32 = Color32::from_rgb(37, 99, 235);
const BACKGROUND: Color32 = Color32::from_rgb(245, 247, 251);
const TEXT: Color32 = Color32::from_rgb(30, 41, 59);
const MUTED: Color32 = Color32::from_rgb(100, 116, 139);
const BORDER: Color32 = Color32::from_rgb(226, 232, 240);
const CONTROL_HEIGHT: f32 = 36.0;
const ROW_HEIGHT: f32 = 40.0;

#[derive(Default)]
struct Row {
    id: String,
    model: String,
    status: String,
    files: [bool; 2],
    success: bool,
    folder: Option<std::path::PathBuf>,
    detail: String,
}

enum Message {
    Progress(DownloadEvent),
    Done(Result<(), String>),
}

pub struct DownloaderApp {
    settings: Settings,
    input: String,
    single: bool,
    rows: Vec<Row>,
    logs: String,
    summary: String,
    receiver: Option<Receiver<Message>>,
    cancel: Option<CancellationToken>,
    completed: usize,
    total: usize,
}

impl DownloaderApp {
    /// 初始化字体、主题和界面状态，并加载用户配置。 / Initialize fonts, theme, and UI state, then load user settings.
    pub fn new(context: &eframe::CreationContext<'_>) -> Self {
        // 复用系统中文字体，减少仓库与分发包中的字体体积。 / Reuse the system CJK font to reduce font data in the repository and distribution.
        let mut fonts = egui::FontDefinitions::default();
        #[cfg(windows)]
        if let Ok(data) = fs::read(
            std::env::var_os("WINDIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| "C:/Windows".into())
                .join("Fonts/msyh.ttc"),
        ) {
            fonts
                .font_data
                .insert("chinese".into(), egui::FontData::from_owned(data).into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, "chinese".into());
            }
        }
        context.egui_ctx.set_fonts(fonts);
        let mut visuals = egui::Visuals::light();
        visuals.selection.bg_fill = BLUE;
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, Color32::WHITE);
        visuals.panel_fill = BACKGROUND;
        visuals.override_text_color = Some(TEXT);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
        visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(239, 246, 255);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, BLUE);
        context.egui_ctx.set_visuals(visuals);
        context.egui_ctx.global_style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(14.0, 6.0);
            style.spacing.interact_size.y = CONTROL_HEIGHT;
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(22.0));
        });
        Self {
            settings: config::load(&config::settings_path()),
            input: String::new(),
            single: false,
            rows: Vec::new(),
            logs: String::new(),
            summary: "等待开始".into(),
            receiver: None,
            cancel: None,
            completed: 0,
            total: 0,
        }
    }

    /// 校验输入模式并将唯一编号追加到任务列表，返回本次识别结果。 / Validate the input mode, append unique IDs to the queue, and return the parsed IDs.
    fn add_ids(&mut self) -> anyhow::Result<Vec<String>> {
        let ids = parse_ids(&self.input);
        if self.single {
            anyhow::ensure!(ids.len() == 1, "单个下载请只输入一个 C 编号");
        }
        if !self.input.trim().is_empty() {
            download::validate_ids(&ids)?;
        }
        for id in &ids {
            if !self.rows.iter().any(|row| &row.id == id) {
                self.rows.push(Row {
                    id: id.clone(),
                    model: "—".into(),
                    status: "等待下载".into(),
                    ..Default::default()
                });
            }
        }
        Ok(ids)
    }

    /// 校验任务并启动后台下载，通过消息通道回传进度与结果。 / Validate the batch and start background downloads, reporting progress and results through a channel.
    fn start(&mut self, ids: Vec<String>, ctx: &egui::Context) {
        if self.cancel.is_some() {
            return;
        }
        if let Err(error) = download::validate_ids(&ids) {
            self.summary = error.to_string();
            return;
        }
        for row in self.rows.iter_mut().filter(|r| ids.contains(&r.id)) {
            row.status = "等待下载".into();
            row.success = false;
            row.files = [false; 2];
            row.folder = None;
            row.detail.clear();
        }
        self.completed = 0;
        self.total = ids.len();
        self.summary = format!("正在下载 {} 个器件", ids.len());
        let request = DownloadRequest {
            ids,
            directory: self.settings.download_directory.clone(),
            parallel: self.settings.parallel,
        };
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?;
                let (events, progress) = mpsc::channel();
                let progress_tx = tx.clone();
                let progress_ctx = ctx.clone();
                // 后台转发进度并唤醒重绘，避免界面线程等待下载。 / Forward progress and request repaints in the background so the UI does not wait for downloads.
                let forward = thread::spawn(move || {
                    for event in progress {
                        let _ = progress_tx.send(Message::Progress(event));
                        progress_ctx.request_repaint();
                    }
                });
                let result = runtime.block_on(download::run(request, events, cancel));
                let _ = forward.join();
                result
            })();
            let _ = tx.send(Message::Done(result.map_err(|error| format!("{error:#}"))));
            ctx.request_repaint();
        });
    }

    /// 非阻塞消费后台事件，更新任务、完成统计及日志。 / Consume background events without blocking and update tasks, completion counts, and logs.
    fn receive(&mut self) {
        let messages: Vec<_> = self
            .receiver
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for message in messages {
            match message {
                Message::Progress(event) => {
                    if let Some(row) = self.rows.iter_mut().find(|row| row.id == event.id) {
                        if let Some(model) = event.model {
                            row.model = model;
                        }
                        row.status = event.status;
                        row.files = event.files;
                        row.folder = event.folder;
                        row.success = event.success;
                        if let Some(detail) = event.detail {
                            row.detail = detail;
                        }
                        self.logs
                            .push_str(&format!("{}：{} {}\n", row.id, row.status, row.detail));
                        if event.finished {
                            self.completed += 1;
                        }
                    }
                }
                Message::Done(result) => {
                    self.cancel = None;
                    self.receiver = None;
                    self.summary = match result {
                        Ok(()) => format!(
                            "本次处理完成 {}/{} · 列表成功 {} · 失败或取消 {}",
                            self.completed,
                            self.total,
                            self.rows.iter().filter(|r| r.success).count(),
                            self.rows
                                .iter()
                                .filter(|r| r.status != "等待下载" && !r.success)
                                .count()
                        ),
                        Err(error) => {
                            self.logs.push_str(&format!("{error}\n"));
                            error
                        }
                    };
                }
            }
        }
        if self.logs.len() > 100_000 {
            let start = self
                .logs
                .char_indices()
                .find(|(i, _)| *i >= 30_000)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.logs.drain(..start);
        }
    }

    /// 读取大小受限的文本清单，解码后提取编号并加入列表。 / Read size-limited text lists, decode their contents, and add parsed IDs to the queue.
    fn import(&mut self) -> anyhow::Result<()> {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("文本 / BOM", &["txt", "csv", "tsv"])
            .pick_files()
        {
            self.input.clear();
            for path in paths {
                anyhow::ensure!(
                    fs::metadata(&path)?.len() <= 10 * 1024 * 1024,
                    "导入文件不能超过 10 MB"
                );
                self.input.push_str(&decode_text(&fs::read(path)?));
                self.input.push('\n');
            }
            self.add_ids()?;
        }
        Ok(())
    }
}

/// 确保目录存在后调用系统文件管理器打开。 / Ensure the directory exists and open it with the system file manager.
fn open_folder(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(windows)]
    let command = "explorer.exe";
    #[cfg(target_os = "macos")]
    let command = "open";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let command = "xdg-open";
    std::process::Command::new(command).arg(path).spawn()?;
    Ok(())
}

/// 按统一尺寸与主次样式创建操作按钮。 / Create an action button with consistent sizing and primary or secondary styling.
fn action_button(text: &str, width: f32, primary: bool) -> egui::Button<'_> {
    let label = RichText::new(text).color(if primary { Color32::WHITE } else { TEXT });
    let button = egui::Button::new(label)
        .min_size(egui::vec2(width, CONTROL_HEIGHT))
        .corner_radius(6);
    if primary {
        button.fill(BLUE).stroke(egui::Stroke::NONE)
    } else {
        button
    }
}

/// 绘制定宽表格单元格并返回交互响应。 / Draw a fixed-width table cell and return its interaction response.
fn table_cell(
    ui: &mut egui::Ui,
    width: f32,
    text: &str,
    color: Color32,
    heading: bool,
    clickable: bool,
) -> egui::Response {
    let mut text = RichText::new(text).color(color);
    if heading {
        text = text.strong();
    }
    ui.allocate_ui_with_layout(
        egui::vec2(width, ROW_HEIGHT),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.add(
                egui::Label::new(text)
                    .truncate()
                    .halign(egui::Align::Center)
                    .sense(if clickable {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    }),
            )
        },
    )
    .inner
}

impl DownloaderApp {
    /// 绘制输入模式、导入操作及保存位置设置。 / Draw input modes, import actions, and download directory settings.
    fn input_panel(&mut self, ui: &mut egui::Ui, busy: bool) {
        ui.vertical_centered(|ui| {
            ui.heading("元器件资源下载");
        });
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(1.0_f32, BORDER))
            .corner_radius(10)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add(action_button("单个下载", 108.0, self.single))
                            .clicked()
                        {
                            self.single = true;
                        }
                        if ui
                            .add(action_button("批量下载", 108.0, !self.single))
                            .clicked()
                        {
                            self.single = false;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !self.single
                                && ui
                                    .add(action_button("导入 TXT / CSV", 144.0, false))
                                    .clicked()
                                && let Err(error) = self.import()
                            {
                                self.summary = error.to_string();
                            }
                        });
                    });
                    let (edit, height) = if self.single {
                        (egui::TextEdit::singleline(&mut self.input), CONTROL_HEIGHT)
                    } else {
                        (
                            egui::TextEdit::multiline(&mut self.input).desired_rows(3),
                            76.0,
                        )
                    };
                    let edit = edit
                        .desired_width(f32::INFINITY)
                        .margin(egui::vec2(12.0, 8.0))
                        .hint_text(if self.single {
                            "输入一个 C 编号，例如：C8734"
                        } else {
                            "输入多个 C 编号，以空格、逗号或换行分隔，例如：C8734、C2040、C25804"
                        });
                    if self.single {
                        ui.add_sized([ui.available_width(), height], edit);
                    } else {
                        egui::Frame::new()
                            .fill(Color32::WHITE)
                            .stroke(egui::Stroke::new(1.0_f32, BORDER))
                            .corner_radius(6)
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("component_input")
                                    .max_height(height)
                                    .min_scrolled_height(height)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.add_sized(
                                            [ui.available_width(), height],
                                            edit.frame(egui::Frame::NONE),
                                        );
                                    });
                            });
                    }
                    ui.horizontal(|ui| {
                        ui.set_min_height(CONTROL_HEIGHT);
                        ui.label(
                            RichText::new(format!(
                                "已识别 {} 个唯一编号",
                                parse_ids(&self.input).len()
                            ))
                            .color(MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(action_button("清空输入", 104.0, false)).clicked() {
                                self.input.clear();
                            }
                            if ui.add(action_button("添加到列表", 112.0, false)).clicked()
                                && let Err(error) = self.add_ids()
                            {
                                self.summary = error.to_string();
                            }
                        });
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        let path_width = (ui.available_width()
                            - 56.0
                            - 88.0
                            - 104.0
                            - ui.spacing().item_spacing.x * 3.0)
                            .max(80.0);
                        ui.add_sized([56.0, CONTROL_HEIGHT], egui::Label::new("保存位置"));
                        let directory = self.settings.download_directory.display().to_string();
                        ui.add_sized(
                            [path_width, CONTROL_HEIGHT],
                            egui::Label::new(&directory)
                                .truncate()
                                .halign(egui::Align::LEFT)
                                .selectable(false),
                        )
                        .on_hover_text(&directory);
                        if ui.add(action_button("更改", 88.0, false)).clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_directory(&self.settings.download_directory)
                                .pick_folder()
                        {
                            let updated = Settings {
                                download_directory: path,
                                ..self.settings.clone()
                            };
                            match config::save(&config::settings_path(), &updated) {
                                Ok(()) => self.settings = updated,
                                Err(error) => self.summary = format!("{error:#}"),
                            }
                        }
                        if ui.add(action_button("打开目录", 104.0, false)).clicked()
                            && let Err(error) = open_folder(&self.settings.download_directory)
                        {
                            self.summary = error.to_string();
                        }
                    });
                });
            });
    }

    /// 绘制任务表格和重试操作，支持双击状态打开输出目录。 / Draw the task table and retry actions, allowing a status double-click to open the output folder.
    fn queue_panel(&mut self, ui: &mut egui::Ui, busy: bool, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.set_min_height(CONTROL_HEIGHT);
            ui.strong(format!("下载列表 · {} 个器件", self.rows.len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let failed = self
                    .rows
                    .iter()
                    .any(|row| !row.success && row.status != "等待下载");
                if ui
                    .add_enabled(!busy && failed, action_button("重试失败项", 112.0, false))
                    .clicked()
                {
                    let ids = self
                        .rows
                        .iter()
                        .filter(|row| !row.success && row.status != "等待下载")
                        .map(|row| row.id.clone())
                        .collect();
                    self.start(ids, ctx);
                }
                if ui
                    .add_enabled(
                        !busy && !self.rows.is_empty(),
                        action_button("清空列表", 104.0, false),
                    )
                    .clicked()
                {
                    self.rows.clear();
                }
            });
        });
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(1.0_f32, BORDER))
            .corner_radius(10)
            .inner_margin(1)
            .show(ui, |ui| {
                let table_height = ui.available_height().max(ROW_HEIGHT + 80.0);
                egui::ScrollArea::vertical()
                    .max_height(table_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        let widths = [
                            width * 0.14,
                            width * 0.34,
                            width * 0.12,
                            width * 0.12,
                            width * 0.28,
                        ];
                        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                        egui::Frame::new()
                            .fill(Color32::from_rgb(241, 245, 249))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    for (heading, width) in
                                        ["C 编号", "具体型号", "PcbDoc", "SchDoc", "状态"]
                                            .into_iter()
                                            .zip(widths)
                                    {
                                        table_cell(ui, width, heading, MUTED, true, false);
                                    }
                                });
                            });
                        for (index, row) in self.rows.iter().enumerate() {
                            let fill = if index % 2 == 0 {
                                Color32::WHITE
                            } else {
                                Color32::from_rgb(248, 250, 252)
                            };
                            egui::Frame::new().fill(fill).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    table_cell(ui, widths[0], &row.id, TEXT, false, false);
                                    table_cell(ui, widths[1], &row.model, TEXT, false, false)
                                        .on_hover_text(&row.model);
                                    for (column, complete) in row.files.into_iter().enumerate() {
                                        table_cell(
                                            ui,
                                            widths[column + 2],
                                            if complete { "完成" } else { "—" },
                                            if complete {
                                                Color32::from_rgb(22, 128, 61)
                                            } else {
                                                MUTED
                                            },
                                            false,
                                            false,
                                        );
                                    }
                                    let status_color = if row.success {
                                        Color32::from_rgb(22, 128, 61)
                                    } else if !row.detail.is_empty() {
                                        Color32::from_rgb(185, 28, 28)
                                    } else {
                                        TEXT
                                    };
                                    let response = table_cell(
                                        ui,
                                        widths[4],
                                        &row.status,
                                        status_color,
                                        false,
                                        true,
                                    )
                                    .on_hover_text(format!("{}\n{}", row.status, row.detail));
                                    if response.double_clicked()
                                        && let Some(folder) = &row.folder
                                        && let Err(error) = open_folder(folder)
                                    {
                                        self.summary = error.to_string();
                                    }
                                });
                            });
                        }
                        if self.rows.is_empty() {
                            ui.allocate_ui_with_layout(
                                egui::vec2(width, table_height - ROW_HEIGHT),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.add_space(
                                        ((table_height - ROW_HEIGHT - 52.0) / 2.0).max(8.0),
                                    );
                                    ui.label(RichText::new("暂无下载任务").size(16.0).color(MUTED));
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new("输入 C 编号后，点击开始下载").color(MUTED),
                                    );
                                },
                            );
                        }
                    });
            });
    }

    /// 绘制启动、取消、进度与日志区域，并处理相应操作。 / Draw start, cancellation, progress, and log controls and handle their actions.
    fn footer(&mut self, ui: &mut egui::Ui, busy: bool, ctx: &egui::Context) {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), CONTROL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let group_width = 152.0 + 88.0 + ui.spacing().item_spacing.x;
                ui.add_space(((ui.available_width() - group_width) / 2.0).max(0.0));
                let title = if self.single {
                    "下载此器件"
                } else {
                    "开始下载"
                };
                if ui
                    .add_enabled(!busy, action_button(title, 152.0, true))
                    .clicked()
                {
                    match self.add_ids() {
                        Ok(input) => {
                            let ids = if self.single {
                                input
                            } else {
                                self.rows
                                    .iter()
                                    .filter(|row| !row.success)
                                    .map(|row| row.id.clone())
                                    .collect()
                            };
                            self.start(ids, ctx);
                        }
                        Err(error) => self.summary = error.to_string(),
                    }
                }
                if ui
                    .add_enabled(busy, action_button("取消", 88.0, false))
                    .clicked()
                {
                    if let Some(cancel) = &self.cancel {
                        cancel.cancel();
                    }
                    self.summary = "正在取消并清理临时文件…".into();
                }
            },
        );
        ui.add_sized(
            [ui.available_width(), 22.0],
            egui::Label::new(RichText::new(&self.summary).color(MUTED))
                .truncate()
                .halign(egui::Align::Center),
        )
        .on_hover_text(&self.summary);
        // 圆角进度条在零进度时仍有最小填充，未开始任务时将填充设为透明。 / Rounded progress bars retain a minimum fill at zero; make it transparent before a batch starts.
        ui.add(
            egui::ProgressBar::new(if self.total == 0 {
                0.0
            } else {
                self.completed as f32 / self.total as f32
            })
            .desired_height(6.0)
            .fill(if self.total == 0 {
                Color32::TRANSPARENT
            } else {
                BLUE
            })
            .animate(busy),
        );
        ui.collapsing("详细日志", |ui| {
            egui::ScrollArea::vertical()
                .max_height(80.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.add(egui::Label::new(&self.logs).wrap());
                });
        });
    }
}

impl eframe::App for DownloaderApp {
    /// 消费后台事件并处理关闭请求，窗口隐藏时也维持任务状态更新。 / Consume background events and handle close requests, keeping task state current while the window is hidden.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive();
        let busy = self.cancel.is_some();
        if busy && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if let Some(cancel) = &self.cancel {
                cancel.cancel();
            }
            self.summary = "正在取消，完成后可关闭窗口".into();
        }
    }

    /// 在根界面中依次布局底部操作区、输入区和任务列表。 / Lay out footer actions, input controls, and the task list within the root UI.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let busy = self.cancel.is_some();
        // 先布局底部操作区，确保任务列表增长时按钮仍可见。 / Lay out the footer first so its actions remain visible as the queue grows.
        egui::Panel::bottom("download_actions")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin {
                        left: 24,
                        right: 24,
                        top: 12,
                        bottom: 16,
                    }),
            )
            .show(ui, |ui| self.footer(ui, busy, &ctx));
        egui::Panel::top("download_input")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin {
                        left: 24,
                        right: 24,
                        top: 18,
                        bottom: 8,
                    }),
            )
            .show(ui, |ui| self.input_panel(ui, busy));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin {
                        left: 24,
                        right: 24,
                        top: 0,
                        bottom: 8,
                    }),
            )
            .show(ui, |ui| self.queue_panel(ui, busy, &ctx));
    }
}
