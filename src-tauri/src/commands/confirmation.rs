//! 待确认的管理员命令：读取、等待变化、批准、拒绝。
//!
//! UI 等待只观察 confirmation 自己的 revision，避免借用无关的 diagnostics
//! revision 造成立即返回和高频 IPC；批准与兑换的安全状态机保持不变。

use serde::Serialize;
use std::time::{Duration, Instant};

use super::error::{UiError, UiResult};
use crate::execution::confirmation;

// 30 秒让空闲窗口保持低唤醒频率；250 ms 只发生在一个本地阻塞任务内部，
// 用于让新确认请求在不改动安全关键 Store 同步模型的前提下及时出现。
const CONFIRMATION_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const CONFIRMATION_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingConfirmationView {
    id: String,
    /// "shell" / "filesystem" / "process"，与账本里的路由同名。
    route: String,
    /// 命令原文，一字不改。摘要过的命令没法让人做判断。
    command: String,
    workdir: Option<String>,
    risk: Vec<String>,
    requested_at_ms: u64,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmationProjection {
    revision: u64,
    entries: Vec<PendingConfirmationView>,
}

#[tauri::command]
pub async fn get_pending_confirmations() -> UiResult<ConfirmationProjection> {
    tauri::async_runtime::spawn_blocking(collect)
        .await
        .map_err(|_| UiError::internal("Ui.ConfirmationReadJoinFailed", "待确认记录后台任务异常"))
}

#[tauri::command]
pub async fn wait_pending_confirmations_change(since_revision: u64) -> UiResult<u64> {
    tauri::async_runtime::spawn_blocking(move || {
        let deadline = Instant::now() + CONFIRMATION_WAIT_TIMEOUT;
        loop {
            let revision = confirmation::revision();
            if revision > since_revision || Instant::now() >= deadline {
                return revision;
            }
            std::thread::sleep(CONFIRMATION_POLL_INTERVAL);
        }
    })
    .await
    .map_err(|_| {
        UiError::internal(
            "Ui.ConfirmationWaitJoinFailed",
            "待确认记录唤醒后台任务异常",
        )
    })
}

fn collect() -> ConfirmationProjection {
    ConfirmationProjection {
        revision: confirmation::revision(),
        entries: confirmation::awaiting()
            .into_iter()
            .map(|entry| PendingConfirmationView {
                id: entry.id,
                route: entry.route,
                command: entry.command,
                workdir: entry.workdir,
                risk: entry.risk,
                requested_at_ms: entry.requested_at_ms,
                expires_at_ms: entry.expires_at_ms,
            })
            .collect(),
    }
}

/// 批准之后令牌才可兑换。模型带令牌重试即执行——这里不去替它重试，
/// 因为那等于后端替模型决定"批准就是立刻跑"，而用户批准的是"可以跑"。
#[tauri::command]
pub async fn approve_administrator_command(id: String) -> UiResult<bool> {
    tauri::async_runtime::spawn_blocking(move || confirmation::approve(&id))
        .await
        .map_err(|_| UiError::internal("Ui.ConfirmationApproveJoinFailed", "批准后台任务异常"))
}

#[tauri::command]
pub async fn reject_administrator_command(id: String) -> UiResult<bool> {
    tauri::async_runtime::spawn_blocking(move || confirmation::reject(&id))
        .await
        .map_err(|_| UiError::internal("Ui.ConfirmationRejectJoinFailed", "拒绝后台任务异常"))
}
