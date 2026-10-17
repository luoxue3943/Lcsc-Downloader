# 项目约定 / Project conventions

- 保持现有 UI 与下载、解析、文档写入、校验模块之间的职责边界；按实际职责增加文件，不创建占位文件或空目录。
  Preserve the boundaries between UI, downloading, parsing, document writing, and validation; add files for concrete responsibilities, not placeholders or empty directories.
- 所有手写注释统一采用单行“中文内容 / English content”，分隔符两侧各留一个空格，不再拆成中英文两行；模块说明使用 `//!`，函数和方法说明使用 `///`，局部说明使用 `//`。 / All handwritten comments must use one line in the form “Chinese content / English content”, with one space on each side of the separator; use `//!` for modules, `///` for functions and methods, and `//` for local notes.
- 所有函数和方法必须有紧邻定义的文档注释，包括公开接口、私有辅助函数及 trait 方法实现；存在属性时，注释放在属性之前。 / Document every function and method immediately above its definition, including public APIs, private helpers, and trait method implementations; place documentation before attributes when present.
- 注释解释设计原因、单位、格式约束、兼容性或异常处理，不复述显而易见的代码，不添加作者、日期、修改流水或逐行翻译。
  Explain rationale, units, format constraints, compatibility, or error handling; avoid obvious narration, author/date banners, change logs, and line-by-line paraphrases.
- 当前项目仅保留运行所需代码与资源，通过 Cargo 命令构建；不为补全目录结构创建测试或构建脚本占位。 / Keep runtime code and resources and build through Cargo commands; do not create test or build-script placeholders merely to fill out a directory structure.
- 包名使用 `lcsc-downloader`，Rust crate 与代码引用使用 `lcsc_downloader`；目录名与包名独立，模块目录使用 `snake_case`。 / Use `lcsc-downloader` as the package name and `lcsc_downloader` for the Rust crate and imports; directory names are independent of package names, and module directories use `snake_case`.
- 修改行为时同步维护相关注释，保留上游数据与版权字段，不为手写注释格式改写自动生成文件。 / Keep comments aligned with behavior, preserve upstream data and license fields, and do not rewrite generated files to enforce handwritten comment formatting.
- 沿用 Rust 模块命名与 `rustfmt` 格式，不套用其他语言的命名规则；依赖版本通过 `Cargo.lock` 保持可复现。
  Follow Rust module naming and `rustfmt` rather than another language's naming rules; retain reproducible dependencies through `Cargo.lock`.
- 提交前运行 `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings` 和 `cargo build --release --locked`。 / Before committing, run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo build --release --locked`.
- 不提交 `target/`、`dist/`、IDE 缓存、临时输出、个人配置和凭据。
  Do not commit `target/`, `dist/`, IDE caches, temporary output, personal settings, or credentials.
