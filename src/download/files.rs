use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};

/// 校验目标目录边界，备份已有内容后发布，失败时尝试回滚。 / Validate destination boundaries, back up existing content, and publish with rollback on failure.
pub(super) fn publish(ready: &Path, destination: &Path, root: &Path) -> Result<()> {
    ensure!(destination.parent() == Some(root), "目标目录超出下载位置");
    if !destination.exists() {
        return fs::rename(ready, destination).context("保存文件失败");
    }
    let metadata = fs::symlink_metadata(destination)?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "目标目录是链接，请选择普通目录"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "目标目录是重解析链接"
        );
    }
    // 替换前备份旧目录，自动恢复失败时继续保留可找回的备份。 / Back up the old directory before replacement and retain it if automatic recovery fails.
    let parent = root.join(".lcsc-backups");
    fs::create_dir_all(&parent)?;
    let container = tempfile::Builder::new()
        .prefix("backup-")
        .tempdir_in(parent)?;
    let backup = container
        .path()
        .join(destination.file_name().context("目标目录没有文件名")?);
    fs::rename(destination, &backup)?;
    if let Err(error) = fs::rename(ready, destination) {
        if let Err(rollback) = fs::rename(&backup, destination) {
            let retained = container.keep();
            anyhow::bail!(
                "保存失败：{error}；恢复失败：{rollback}；旧文件保留在 {}",
                retained.display()
            );
        }
        return Err(error.into());
    }
    let _ = container.keep();
    Ok(())
}
