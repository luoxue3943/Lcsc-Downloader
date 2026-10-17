//! 执行可取消批次，每个器件整体发布两份通过校验的 Doc。 / Run cancellable batches and publish two validated Doc files together for each component.
mod client;
mod files;

use crate::{
    input::safe_name,
    validation::{validate_complete, validate_documents},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, mpsc::Sender},
    time::Duration,
};
use tokio::{sync::Semaphore, task::JoinSet};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct DownloadRequest {
    pub ids: Vec<String>,
    pub directory: PathBuf,
    pub parallel: usize,
}
#[derive(Clone, Debug)]
pub struct DownloadEvent {
    pub id: String,
    pub model: Option<String>,
    pub status: String,
    /// 完成标记的顺序固定为 PcbDoc、SchDoc，与界面列对应。 / Completion flags follow the fixed PcbDoc, SchDoc order used by the UI columns.
    pub files: [bool; 2],
    /// 每个唯一器件仅报告一次终态，避免界面重复累计完成数。 / Report one terminal event per unique component to prevent duplicate completion counts in the UI.
    pub finished: bool,
    pub success: bool,
    pub folder: Option<PathBuf>,
    pub detail: Option<String>,
}
impl DownloadEvent {
    /// 为指定器件创建尚未完成的等待事件。 / Create a pending, unfinished event for the given component.
    fn new(id: String) -> Self {
        Self {
            id,
            model: None,
            status: "等待下载".into(),
            files: [false; 2],
            finished: false,
            success: false,
            folder: None,
            detail: None,
        }
    }

    /// 更新状态并发送事件快照，接收端关闭时忽略发送失败。 / Update the status and send an event snapshot, ignoring send failure if the receiver has closed.
    fn report(&mut self, status: &str, tx: &Sender<Self>) {
        self.status = status.into();
        let _ = tx.send(self.clone());
    }
}

/// 去重并限流执行可取消的批次，为各器件汇报终态。 / Deduplicate and limit a cancellable batch, reporting a terminal state for each component.
pub async fn run(
    request: DownloadRequest,
    tx: Sender<DownloadEvent>,
    cancel: CancellationToken,
) -> Result<()> {
    validate_ids(&request.ids)?;
    std::fs::create_dir_all(&request.directory).context("创建下载目录失败")?;
    let root = std::fs::canonicalize(&request.directory)?;
    let gate = Arc::new(Semaphore::new(request.parallel.clamp(1, 4)));
    let client = client::Client::new()?;
    let mut seen = HashSet::new();
    let mut jobs = JoinSet::new();
    for id in request.ids.into_iter().filter(|id| seen.insert(id.clone())) {
        let (gate, client, root, tx, cancel) = (
            gate.clone(),
            client.clone(),
            root.clone(),
            tx.clone(),
            cancel.clone(),
        );
        jobs.spawn(async move {
            let mut event = DownloadEvent::new(id);
            let permit = tokio::select! {
                biased;
                _ = cancel.cancelled() => None,
                permit = gate.acquire_owned() => Some(permit)
            };
            if permit.is_none() {
                event.finished = true;
                event.report("已取消", &tx);
                return;
            }
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(anyhow::anyhow!("已取消")),
                result = tokio::time::timeout(
                    Duration::from_secs(300),
                    one(&client, &root, &mut event, &tx, &cancel)
                ) => result.context("下载超时，请稍后重试").and_then(|r| r),
            };
            event.finished = true;
            match result {
                Ok(()) => {
                    event.success = true;
                    let status = if event.status == "已完整，跳过" {
                        "已完整，跳过"
                    } else {
                        "下载完成"
                    };
                    event.report(status, &tx);
                }
                Err(error) => {
                    event.detail = Some(format!("{error:#}"));
                    event.report(
                        if cancel.is_cancelled() {
                            "已取消"
                        } else {
                            "失败 / 不完整"
                        },
                        &tx,
                    );
                }
            }
        });
    }
    while let Some(result) = jobs.join_next().await {
        result.context("下载线程异常退出")?;
    }
    Ok(())
}

/// 查询器件并复用完整输出，必要时转换、校验和发布两份文档。 / Query the component and reuse complete output, otherwise converting, validating, and publishing two documents.
async fn one(
    client: &client::Client,
    root: &std::path::Path,
    event: &mut DownloadEvent,
    tx: &Sender<DownloadEvent>,
    cancel: &CancellationToken,
) -> Result<()> {
    event.report("查询型号", tx);
    let product = client.product(&event.id).await?;
    let model = product["model"].as_str().context("没有型号")?;
    let name = safe_name(model)?;
    event.model = Some(model.to_owned());
    let destination = root.join(format!("{}_{}", event.id, name));
    if destination.exists() && validate_complete(&destination, &name, &event.id).is_ok() {
        event.files = [true; 2];
        event.folder = Some(destination);
        event.status = "已完整，跳过".into();
        return Ok(());
    }
    event.report("获取符号与封装", tx);
    let mut component = client.component(&product).await?;
    // 文档内型号与安全文件名保持一致，以满足后续身份校验。 / Keep internal model names consistent with safe filenames for subsequent identity validation.
    component.model = name.clone();
    event.report("生成 SchDoc / PcbDoc", tx);
    let documents = crate::altium::render(&component)?;
    validate_documents(&documents.schematic, &documents.pcb, &name, &event.id)?;
    ensure!(!cancel.is_cancelled(), "已取消");
    // 将两份文档暂存于目标文件系统，校验通过后整体发布。 / Stage both documents on the destination filesystem and publish them together after validation.
    let staging = tempfile::Builder::new()
        .prefix(".lcsc-tmp-")
        .tempdir_in(root)?;
    let ready = staging.path().join("ready");
    std::fs::create_dir(&ready)?;
    std::fs::write(ready.join(format!("{name}.SchDoc")), documents.schematic)?;
    event.files[1] = true;
    std::fs::write(ready.join(format!("{name}.PcbDoc")), documents.pcb)?;
    event.files[0] = true;
    event.report("校验两个 Doc 文件", tx);
    validate_complete(&ready, &name, &event.id)?;
    ensure!(!cancel.is_cancelled(), "已取消");
    event.report("保存文件", tx);
    files::publish(&ready, &destination, root)?;
    event.folder = Some(destination);
    Ok(())
}

/// 要求任务列表非空，且各编号已符合规范化的 C 编号格式。 / Require a nonempty task list whose IDs already follow the normalized C ID format.
pub fn validate_ids(ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        bail!("请先输入立创 C 编号");
    }
    ensure!(
        ids.iter()
            .all(|id| crate::input::parse_ids(id) == [id.clone()]),
        "无效的立创编号"
    );
    Ok(())
}
