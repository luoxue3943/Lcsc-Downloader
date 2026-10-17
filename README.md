# 立创元器件 Doc 下载器

原生 Rust 桌面工具，根据立创 C 编号生成独立的 Altium ASCII 文档。每个器件只输出 SchDoc 和 PcbDoc 两个文件，不下载或输出 STEP、SchLib、PcbLib。资源获取、源数据解析及 Doc 写入由本项目实现，无需 EDA 转换引擎依赖。

## 使用

直接运行编译生成的 `target/release/lcsc-downloader.exe`。

`assets/app.png` 是窗口图标，`assets/app.ico` 是 Windows exe 图标。PNG 在编译时嵌入，ICO 由 Cargo 自动执行的 `build.rs` 写入程序资源；运行时不需要附带 assets 文件夹。

从源码运行：

```powershell
cargo run --release --locked
```

或者编译后运行 target/release/lcsc-downloader.exe：

```powershell
cargo build --release --locked
```

界面支持单个下载、批量编号、TXT/CSV/TSV 导入、去重、失败重试、取消和下载位置记忆。默认使用 Windows“下载”文件夹，配置位于 %LOCALAPPDATA%/LcscDownloader/settings.json。

```text
下载目录/
└── C8734_STM32F103C8T6/
    ├── STM32F103C8T6.SchDoc
    └── STM32F103C8T6.PcbDoc
```

SchDoc 放置器件完整的符号及所有子单元，PcbDoc 放置对应封装及二维图形，可以在 Altium 中编辑或复制到自己的工程。两份文档不包含三维模型，也不自动生成应用电路或布线。

## 转换与校验

- C 编号精确匹配，不用相似器件替代。
- 读取专业版数组行格式的 SYMBOL 1.x 和 FOOTPRINT 1.x。当前不支持标准版字符串图元或专业版 V3 日志格式，遇到这些格式会明确报错。
- 保留引脚编号、名称、电气类型、方向和连接端点；多单元器件全部放入 SchDoc。
- 保留支持的焊盘尺寸、坐标、旋转、孔径、金属化、阻焊及助焊扩展。支持矩形、圆形/椭圆/长圆、八边形焊盘及居中圆孔/长圆孔。
- 支持常见符号矩形、椭圆、折线、圆弧、文字，以及封装折线、圆弧和单轮廓填充；按源图层映射到铜、丝印、阻焊、助焊及机械层。
- 尚未支持的自定义焊盘、逐层焊盘堆叠、偏心钻孔、带内孔填充或未知图元会使该器件导出失败，避免静默丢失图元。
- 验证文档头、记录数量、父对象索引、子单元数量、引脚/焊盘数量与编号对应关系。文件名使用具体型号，非法字符替换为下划线。
- 两份文档全部验证通过后整体保存；已有目录只有重新验证通过且恰好包含两个 Doc 才会跳过。需要替换时，旧目录保留在 .lcsc-backups，保存失败尝试回滚。
- 验证失败或取消的中间文件由临时目录自动清理。网络请求有超时和有限重试，默认同时处理两个器件。

结构及数字校验不等同于在 Altium 中检查所有器件的视觉与电气兼容性；使用前应核对自己的器件引脚、焊盘与尺寸。

## 项目结构

```text
src/
├── main.rs              # 桌面入口 / Desktop entry point
├── lib.rs               # 核心库入口 / Core library entry point
├── ui.rs                # 双文件任务列表 / Two-document task UI
├── config.rs            # 保存位置记忆 / Download directory settings
├── input.rs             # 编号与导入文本处理 / ID parsing and text import
├── model.rs             # 二维器件模型 / Format-independent 2D component model
├── source.rs            # 原始格式解析 / Source format parsing
├── validation.rs        # 文档及引脚与焊盘校验 / Document and pin-to-pad validation
├── altium/
│   ├── mod.rs           # ASCII 记录与公共工具 / ASCII records and shared utilities
│   ├── sch.rs           # SchDoc 写入 / SchDoc writing
│   └── pcb.rs           # PcbDoc 写入 / PcbDoc writing
└── download/
    ├── mod.rs           # 批量、并发与取消 / Batches, concurrency, and cancellation
    ├── client.rs        # HTTP 查询与资源获取 / HTTP queries and resource fetching
    └── files.rs         # 目录替换、备份与回滚 / Directory publishing, backup, and rollback
```

`assets/` 保存窗口图标与 Windows exe 图标，`build.rs` 仅处理 Windows 图标资源。源码按现有职责划分，不包含测试目录或测试样例。

## 开发约定

项目使用 Rust 2024 edition；升级到 eframe 0.37 后，最低 Rust 版本为 1.95。提交 `Cargo.lock` 以固定依赖解析结果。

Cargo 包名为 `lcsc-downloader`，Rust 库名及代码引用为 `lcsc_downloader`。Cargo 默认将包名中的连字符转换为库名中的下划线，见 [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-name-field)。外层项目目录 `Lcsc-Downloader` 与包名独立，保留现名；Cargo 允许通过 `--name` 单独指定包名，见 [cargo new](https://doc.rust-lang.org/cargo/commands/cargo-new.html)。`src/` 及模块目录沿用 Cargo 布局和 Rust 模块命名约定。

所有手写注释统一采用单行“中文内容 / English content”，分隔符两侧各留一个空格。所有函数和方法（包括私有辅助函数和 trait 方法实现）均使用 `///` 文档注释，模块说明使用 `//!`，局部说明使用 `//`。注释说明职责、关键约束和错误处理，避免逐行复述代码；详细规则见 [AGENTS.md](AGENTS.md)。

文本文件统一使用 UTF-8 与 LF；规则由 `.gitattributes` 管理。`target/`、`dist/` 和 `.idea/` 为本地构建或开发文件，不纳入项目源码。

## 验证命令

```powershell
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

## 数据来源与格式参考

元器件数据来源：嘉立创EDA/EasyEDA官方库。[嘉立创EDA](https://lceda.cn/) · [EasyEDA](https://easyeda.com/)。下载数据保留上游版权字段，生成文档中保留来源与原始版权说明。

独立写入器参考公开的嘉立创文件格式文档、Altium ASCII 文档资料以及 KiCad 的格式说明。具体出处见 THIRD-PARTY-NOTICES.md。
