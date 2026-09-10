import type { ActivityEntry } from "./api";

const actionText: Record<string, string> = {
  filesystem: "文件",
  exec_command: "运行命令",
  git_workflow: "Git",
  agent_workflow: "工程",
  document_workflow: "文档",
  view_image: "查看图片",
  command_control: "命令",
  task_control: "任务",
  workspace_context: "读取环境",
  elevated_exec: "管理员操作",
  shell: "管理员命令",
  process: "管理员程序",
};

const operationText: Record<string, Record<string, string>> = {
  document_workflow: { inspect: "读取", search: "搜索", create: "创建", edit: "写入", convert: "转换", rebuild: "重建" },
  filesystem: {
    list: "列出", stat: "查看", read: "读取", write: "写入", replace: "替换", patch: "修改",
    search: "搜索", search_content: "内容搜索", copy: "复制", move: "移动", delete: "删除", hash: "校验",
  },
  git_workflow: { status: "状态", diff: "差异", log: "历史", show: "查看", blame: "追溯" },
  command_control: { adopt: "接管", poll: "查看", read: "读取", write: "输入", kill: "终止" },
  task_control: { list: "列表", get: "查看", cancel: "取消" },
  agent_workflow: {
    diagnose: "诊断", bugfix: "修复", feature: "开发", refactor: "重构", test_failure: "修复测试",
    build_release: "构建", document: "文档", resume: "继续", custom: "执行",
  },
  elevated_exec: { shell: "命令", filesystem: "文件", process: "程序" },
};

const outcomeText: Record<string, string> = {
  success: "完成",
  completed: "完成",
  failed: "失败",
  blocked: "已阻止",
  cancelled: "已取消",
  timed_out: "超时",
  lost: "已丢失",
  broker_failed: "失败",
  awaiting_confirmation: "等待确认",
  confirmation_awaiting: "等待确认",
  confirmation_expired: "确认已超时",
  confirmation_mismatch: "确认不匹配",
  confirmation_unknown: "确认无效",
  elevation_unavailable: "未授权",
};

export const riskText: Record<string, string> = {
  bulk_delete: "批量删除",
  disk_format: "磁盘格式化",
  registry_destruction: "注册表删除",
  security_controls: "修改安全设置",
  boot_and_recovery: "修改启动与恢复",
  administrator_accounts: "管理员账户",
};

export function activityAction(entry: ActivityEntry): string {
  const action = actionText[entry.action] ?? "其他操作";
  if (!entry.operation) return action;
  const operation = operationText[entry.action]?.[entry.operation];
  return operation ? action + " " + operation : action;
}

export function activityOutcome(entry: ActivityEntry): string | null {
  if (!entry.outcome) return null;
  return outcomeText[entry.outcome] ?? "状态未知";
}

export type ActivityTone = "positive" | "neutral" | "negative";

const negativeOutcomes = new Set([
  "failed", "timed_out", "lost", "broker_failed", "confirmation_mismatch", "confirmation_unknown", "elevation_unavailable",
]);

export function activityTone(entry: ActivityEntry): ActivityTone {
  if (entry.outcome === "success" || entry.outcome === "completed") return "positive";
  if (entry.outcome && negativeOutcomes.has(entry.outcome)) return "negative";
  return "neutral";
}

export function activityRiskText(risk: string): string {
  return riskText[risk] ?? "需注意";
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
  if (seconds < 60) return String(seconds) + "S";
  return String(Math.floor(seconds / 60)) + "分" + String(seconds % 60) + "S";
}

const errorCodeText: Record<string, string> = {
  InvalidRequest: "请求无效",
  Unavailable: "服务不可用",
  Denied: "权限不足",
  Timeout: "执行超时",
  Cancelled: "已取消",
  ExecutionFailed: "执行失败",
  Unknown: "未知错误",
  ProcessTimedOut: "执行超时",
  ProcessCancelled: "已取消",
  SessionUnavailable: "会话不可用",
  WorkspaceDenied: "工作区拒绝",
};

const causeText: Record<string, string> = {
  workspace_denied: "工作区拒绝",
  capability_denied: "权限不足",
  policy_denied: "策略阻止",
  task_not_owned: "任务不归属",
  elevated_operation_not_reviewed: "管理员操作未审查",
  privileged_route_unavailable: "管理员通道不可用",
  elevation_required: "需要管理员权限",
  process_timed_out: "执行超时",
  operation_timed_out: "请求超时",
  process_failed: "执行失败",
  session_unavailable: "会话不可用",
  protocol_mismatch: "协议不兼容",
  runtime_unavailable: "运行时不可用",
  capability_unavailable: "能力不可用",
  runtime_capability_mismatch: "能力不兼容",
  output_truncated: "输出被截断",
  internal: "内部错误",
};

const outcomesWithoutErrorReason = new Set([
  "success",
  "completed",
  "cancelled",
  "awaiting_confirmation",
  "confirmation_awaiting",
]);

export function activityErrorReason(entry: ActivityEntry): string | null {
  if (entry.outcome && outcomesWithoutErrorReason.has(entry.outcome)) return null;
  if (entry.cause && causeText[entry.cause]) return causeText[entry.cause];
  if (entry.errorCode) return errorCodeText[entry.errorCode] ?? "执行失败";
  if (entry.exitCode != null && entry.exitCode !== 0) return `退出码 ${entry.exitCode}`;
  return null;
}

export function activityDetailDuration(durationMs: number | null): string | null {
  if (durationMs == null) return null;
  if (durationMs < 1000) return `${durationMs}ms`;
  return activityDuration(durationMs);
}

export function activitySourceText(source: ActivityEntry["source"]): string {
  return source === "administrator" ? "管理员命令" : "工具调用";
}
