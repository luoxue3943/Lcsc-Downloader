# 项目约定 / Project conventions

- 保持现有 UI 与下载、解析、文档写入、校验模块之间的职责边界；按实际职责增加文件，不创建占位文件或空目录。
  Preserve the boundaries between UI, downloading, parsing, document writing, and validation; add files for concrete responsibilities, not placeholders or empty directories.
- 所有手写注释统一采用单行“中文内容 / English content”，分隔符两侧各留一个空格，不再拆成中英文两行；模块说明使用 `//!`，函数和方法说明使用 `///`，局部说明使用 `//`。 / All handwritten comments must use one line in the form “Chinese content / English content”, with one space on each side of the separator; use `//!` for modules, `///` for functions and methods, and `//` for local notes.
- 所有函数和方法必须有紧邻定义的文档注释，包括公开接口、私有辅助函数、trait 方法实现、测试函数以及构建脚本入口；存在属性时，注释放在属性之前。 / Document every function and method immediately above its definition, including public APIs, private helpers, trait method implementations, tests, and build-script entry points; place documentation before attributes when present.
- 注释解释设计原因、单位、格式约束、兼容性或异常处理，不复述显而易见的代码，不添加作者、日期、修改流水或逐行翻译。
  Explain rationale, units, format constraints, compatibility, or error handling; avoid obvious narration, author/date banners, change logs, and line-by-line paraphrases.
- 修改行为时同步维护对应注释与测试；保留真实样例的上游数据和版权字段，不改写生成工具或上游维护的文件来满足手写注释格式。
  Keep comments and tests aligned with behavior; preserve upstream fixture data and license fields, and do not rewrite generated or upstream-managed files to enforce handwritten comment formatting.
- 沿用 Rust 模块命名与 `rustfmt` 格式，不套用其他语言的命名规则；依赖版本通过 `Cargo.lock` 保持可复现。
  Follow Rust module naming and `rustfmt` rather than another language's naming rules; retain reproducible dependencies through `Cargo.lock`.
- 提交前运行 `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`；联网回归测试显式运行。
  Before committing, run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and `cargo test --locked`; run live regression tests explicitly.
- 不提交 `target/`、`dist/`、IDE 缓存、临时输出、个人配置和凭据。
  Do not commit `target/`, `dist/`, IDE caches, temporary output, personal settings, or credentials.
