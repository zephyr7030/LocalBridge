import type { ActivityEntry } from "./api";

// 工具名是给模型看的，这里翻成用户看得懂的动作。
const actionText: Record<string, string> = {
  filesystem: "文件",
  exec_command: "运行命令",
  git_workflow: "版本操作",
  agent_workflow: "工程任务",
  document_workflow: "文档",
  view_image: "查看图片",
  command_control: "命令控制",
  task_control: "任务控制",
  workspace_context: "读取环境",
  shell: "管理员命令",
  process: "管理员程序",
};

// 只保留用户需要区分的结果，其余一律算完成——把内部状态机原样搬到
// 界面上是上一版的毛病。
const outcomeText: Record<string, string> = {
  completed: "完成",
  failed: "失败",
  blocked: "已阻止",
  cancelled: "已取消",
  timed_out: "超时",
  lost: "已丢失",
  broker_failed: "失败",
  elevation_unavailable: "未授权",
};

export const riskText: Record<string, string> = {
  bulk_delete: "批量删除",
  disk_format: "磁盘格式化",
  registry_destruction: "注册表删除",
  security_controls: "拆防护",
  boot_and_recovery: "断退路",
  administrator_accounts: "管理员账户",
};

export function activityAction(entry: ActivityEntry): string {
  return actionText[entry.action] ?? entry.action;
}

export function activityOutcome(entry: ActivityEntry): string | null {
  if (!entry.outcome) return null;
  return outcomeText[entry.outcome] ?? entry.outcome;
}

export type ActivityTone = "ok" | "bad" | "pending";

export function activityTone(entry: ActivityEntry): ActivityTone {
  if (!entry.outcome) return "pending";
  return entry.outcome === "completed" ? "ok" : "bad";
}

export function activityTime(timestampMs: number): string {
  return new Date(timestampMs).toLocaleTimeString("zh-CN", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function activityDuration(durationMs: number | null): string | null {
  if (durationMs == null || durationMs < 1000) return null;
  const seconds = Math.round(durationMs / 1000);
  if (seconds < 60) return `${seconds}S`;
  return `${Math.floor(seconds / 60)}分${seconds % 60}S`;
}
