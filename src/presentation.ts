import type { AccessCode, CurrentActivityProjection, LastActivityProjection, LastToolProjection, PrivilegeCode, ServiceCode, TaskProjection, UpdateProjection } from "./bridge";
export const uiText = { dashboard: "主控界面", settings: "设置", diagnostics: "诊断" } as const;
export const accessText: Record<AccessCode, string> = { edit: "编辑模式", full: "完整模式", admin: "管理员模式" };
export const privilegeText: Record<PrivilegeCode, string> = { off: "未启用", requested: "等待授权", awaiting: "等待系统授权", active: "已启用", fault: "故障" };
export const serviceText: Record<ServiceCode, string> = { off: "未启动", starting: "正在启动", online: "已连接", recovering: "正在恢复", fault: "连接失败" };
export type ServiceVisualState = "ready" | "starting" | "fault" | "unknown";
export const serviceVisualState: Record<ServiceCode, ServiceVisualState> = { off: "unknown", starting: "starting", online: "ready", recovering: "starting", fault: "fault" };
const taskKindText = { read: "读取文件", search: "搜索代码", modify: "修改文件", command: "运行命令", git: "版本操作", build: "构建项目", test: "运行测试", admin: "管理员操作", other: "处理任务" } as const;
const activityKindText = { read: "读取文件", search: "搜索代码", modify: "修改文件", command: "运行命令", git: "Git 操作", build: "构建项目", test: "运行测试", admin: "管理员操作", other: "任务执行中" } as const;
const workflowStepText: Record<string, string> = { prepare: "准备", edit: "修改", verify: "验证", persist: "保存", resume: "恢复" };
const taskStateText = { idle: "空闲", running: "", waiting: "等待授权", blocked: "已阻止", failed: "执行失败", cancelled: "已取消" } as const;
function formatElapsed(ms: number): string { const seconds = Math.floor(Math.max(0, ms) / 1000); if (seconds < 60) return `${seconds}S`; const minutes = Math.floor(seconds / 60); if (minutes < 60) return `${minutes}分钟`; return `${Math.floor(minutes / 60)}小时`; }
export function formatLastToolAge(ms: number): string { const bounded = Math.max(0, ms); if (bounded < 60_000) return `${Math.floor(bounded / 1000)}S前`; if (bounded < 3_600_000) return `${Math.floor(bounded / 60_000)}分钟前`; if (bounded < 86_400_000) return "大于1小时"; return `大于${Math.floor(bounded / 86_400_000)}天`; }
export function lastToolText(tool: LastToolProjection): string { const detail = tool.summary ? `  ${tool.summary}` : ""; return `上次执行工具：${taskKindText[tool.kind]}${detail}`; }
export function taskText(task: TaskProjection | null): string { if (!task || task.state === "idle") return "空闲"; const parts: string[] = [taskKindText[task.kind]]; if (task.summary) parts.push(task.summary); if (task.state === "running" && task.elapsedMs != null) parts.push(formatElapsed(task.elapsedMs)); const state = taskStateText[task.state]; if (state) parts.push(state); return parts.join("  "); }

const lastCommandStatusText = { completed: "成功", failed: "失败", cancelled: "已取消", timed_out: "已超时", lost: "已丢失" } as const;
export function currentActivityText(activity: CurrentActivityProjection | null): string {
  if (activity?.state === "cancelling") return "正在取消…";
  if (activity?.state === "waiting_input") return "等待输入…";
  if (activity?.state === "waiting") return "任务等待继续";
  if (activity) return `${activityKindText[activity.kind]}…`;
  return "空闲";
}
export function currentActivityDetail(activity: CurrentActivityProjection | null): string | null {
  if (!activity) return null;
  if (activity.summary) return activity.summary;
  const step = activity.step ? workflowStepText[activity.step] : null;
  if (!step) return null;
  if (activity.progressCurrent != null && activity.progressTotal != null) return `${step} ${activity.progressCurrent}/${activity.progressTotal}`;
  return step;
}
export function currentActivityElapsed(activity: CurrentActivityProjection | null): string | null { return activity?.state === "running" && activity.elapsedMs != null ? formatElapsed(activity.elapsedMs) : null; }
export function lastActivityAction(activity: LastActivityProjection): string { return activityKindText[activity.kind]; }
export function lastActivityOutcome(activity: LastActivityProjection): string { return lastCommandStatusText[activity.outcome]; }
export function updateStatusText(update: UpdateProjection | null): string {
  if (!update) return "正在读取版本";
  switch (update.state) {
    case "source_unavailable": return `当前版本 ${update.currentVersion} · 发布源不可用`;
    case "idle": return `当前版本 ${update.currentVersion} · 尚未检查`;
    case "checking": return `当前版本 ${update.currentVersion} · 正在检查更新`;
    case "current": return `当前版本 ${update.currentVersion} · 已是最新版本`;
    case "available": return `发现新版本 ${update.latestVersion ?? ""}`.trim();
    case "failed": return `当前版本 ${update.currentVersion} · 检查失败`;
  }
}
