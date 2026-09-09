//! 活动流：这台机器上刚刚发生了什么。
//!
//! 界面此前只能显示"当前在做什么"和"上一次做了什么"，而后端一直在记录
//! 完整的调用序列，只是没有任何一条通路把它送到屏幕上。这里补上那条通路。
//!
//! 两个来源合并：MCP 工具调用（全局诊断存储）与管理员命令账本（磁盘）。
//! 工具调用的开始与结束是两条事件，按 request_id 配对——开始那条知道做了
//! 什么，结束那条知道结果如何。

use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;

use super::error::{UiError, UiResult};
use crate::diagnostics::{
    RequestDiagnosticEvent, RequestDiagnosticKind, diagnostics_log_revision,
    recent_request_diagnostics, wait_diagnostics_log_change_after,
};

const FEED_LIMIT: usize = 60;
const LEDGER_LIMIT: usize = 40;
// Bound idle long-polls so a lost wake or shutdown cannot strand a frontend request.
const ACTIVITY_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    /// "tool" 或 "administrator"：后者是以管理员令牌执行的，值得区别对待。
    source: &'static str,
    timestamp_ms: u64,
    action: String,
    operation: Option<String>,
    target: Option<String>,
    outcome: Option<String>,
    error_code: Option<String>,
    duration_ms: Option<u64>,
    exit_code: Option<u32>,
    risk: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityProjection {
    log_revision: u64,
    entries: Vec<ActivityEntry>,
}

#[tauri::command]
pub async fn get_activity() -> UiResult<ActivityProjection> {
    tauri::async_runtime::spawn_blocking(collect_activity)
        .await
        .map_err(|_| UiError::internal("Ui.ActivityReadJoinFailed", "活动记录后台任务异常"))
}

#[tauri::command]
pub async fn wait_activity_change(since_revision: u64) -> UiResult<u64> {
    tauri::async_runtime::spawn_blocking(move || {
        wait_diagnostics_log_change_after(since_revision, ACTIVITY_WAIT_TIMEOUT)
    })
    .await
    .map_err(|_| UiError::internal("Ui.ActivityWaitJoinFailed", "活动记录唤醒后台任务异常"))
}

fn collect_activity() -> ActivityProjection {
    let log_revision = diagnostics_log_revision();
    let mut entries = tool_entries();
    entries.extend(administrator_entries());
    entries.sort_by(|left, right| right.timestamp_ms.cmp(&left.timestamp_ms));
    entries.truncate(FEED_LIMIT);
    ActivityProjection {
        log_revision,
        entries,
    }
}

fn tool_entries() -> Vec<ActivityEntry> {
    let events = recent_request_diagnostics();
    // 结束事件先收集起来，再回填到对应的开始事件上。开始事件带工具名和目标，
    // 结束事件带结果和耗时，只有合起来才是一条能读的记录。
    let mut ends: HashMap<&str, &RequestDiagnosticEvent> = HashMap::new();
    for event in &events {
        if event.kind == RequestDiagnosticKind::End {
            ends.entry(event.request_id.as_str()).or_insert(event);
        }
    }
    events
        .iter()
        .filter(|event| event.kind == RequestDiagnosticKind::Start && !event.tool.is_empty())
        .map(|start| {
            let end = ends.get(start.request_id.as_str());
            ActivityEntry {
                source: "tool",
                timestamp_ms: start.timestamp_ms,
                action: start.tool.clone(),
                operation: start.operation.clone(),
                target: start.target.clone(),
                outcome: end.and_then(|end| end.outcome.clone()),
                error_code: end.and_then(|end| end.error_code.clone()),
                duration_ms: end.and_then(|end| end.duration_ms),
                exit_code: None,
                risk: Vec::new(),
            }
        })
        .collect()
}

fn administrator_entries() -> Vec<ActivityEntry> {
    crate::audit::recent_administrator_commands(LEDGER_LIMIT)
        .into_iter()
        .map(|entry| {
            let string = |key: &str| {
                entry
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            };
            ActivityEntry {
                source: "administrator",
                timestamp_ms: entry
                    .get("timestamp_ms")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default(),
                action: string("route").unwrap_or_else(|| "shell".to_string()),
                operation: None,
                target: string("command"),
                outcome: string("outcome"),
                error_code: None,
                duration_ms: entry.get("duration_ms").and_then(serde_json::Value::as_u64),
                exit_code: entry
                    .get("exit_code")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|code| u32::try_from(code).ok()),
                risk: entry
                    .get("risk")
                    .and_then(serde_json::Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect()
}
