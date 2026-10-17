use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Settings {
    #[serde(alias = "DownloadDirectory")]
    pub download_directory: PathBuf,
    #[serde(default = "default_parallel", alias = "Parallel")]
    pub parallel: usize,
}

/// 为未配置并发数的设置提供默认值二。 / Supply a concurrency default of two when no value is configured.
fn default_parallel() -> usize {
    2
}

impl Default for Settings {
    /// 优先选择系统下载目录，并设置默认并发数。 / Prefer the system downloads directory and initialize the default concurrency.
    fn default() -> Self {
        Self {
            download_directory: dirs::download_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Downloads")),
            parallel: default_parallel(),
        }
    }
}

/// 定位本地应用数据下的配置文件，必要时回退到临时目录。 / Locate the settings file under local application data, falling back to the temporary directory.
pub fn settings_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("LcscDownloader/settings.json")
}

/// 加载兼容旧字段的配置，约束并发范围，无效配置回退为默认值。 / Load settings with legacy field aliases, clamp concurrency, and fall back to defaults when invalid.
pub fn load(path: &Path) -> Settings {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .filter(|s| s.download_directory.is_absolute())
        .map(|s| Settings {
            parallel: s.parallel.clamp(1, 4),
            ..s
        })
        .unwrap_or_default()
}

/// 完整写入并同步临时文件后替换配置，失败时返回错误。 / Write and sync a temporary file before replacing settings, returning any failure.
pub fn save(path: &Path, settings: &Settings) -> Result<()> {
    let parent = path.parent().context("配置路径没有父目录")?;
    fs::create_dir_all(parent)?;
    // 先在同目录完成并同步临时文件，再替换配置以避免截断。 / Complete and sync a same-directory temporary file before replacing settings to avoid truncation.
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&serde_json::to_vec_pretty(settings)?)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).context("保存下载位置失败")?;
    Ok(())
}
