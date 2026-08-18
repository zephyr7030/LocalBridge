import type { AccessCode, CurrentCommandProjection, CurrentWorkflowProjection, LastCommandProjection, LastToolProjection, PrivilegeCode, ServiceCode, TaskProjection } from "./bridge";
export const uiText = { dashboard: "主控界面", settings: "设置", diagnostics: "诊断" } as const;
export const accessText: Record<AccessCode, string> = { edit: "编辑模式", full: "完整模式", admin: "管理员模式" };
export const privilegeText: Record<PrivilegeCode, string> = { off: "未启用", requested: "等待授权", awaiting: "等待系统授权", active: "已启用", fault: "故障" };
export const serviceText: Record<ServiceCode, string> = { off: "未启动", starting: "正在启动", online: "已连接", recovering: "正在恢复", fault: "连接失败" };
export type ServiceVisualState = "ready" | "starting" | "fault" | "unknown";
export const serviceVisualState: Record<ServiceCode, ServiceVisualState> = { off: "unknown", starting: "starting", online: "ready", recovering: "starting", fault: "fault" };
const taskKindText = { read: "读取文件", search: "搜索代码", modify: "修改文件", command: "运行命令", git: "版本操作", build: "构建项目", test: "运行测试", admin: "管理员操作", other: "处理任务" } as const;
const taskStateText = { idle: "空闲", running: "", waiting: "等待授权", blocked: "已阻止", failed: "执行失败", cancelled: "已取消" } as const;
function formatElapsed(ms: number): string { const seconds = Math.floor(Math.max(0, ms) / 1000); if (seconds < 60) return `${seconds}S`; const minutes = Math.floor(seconds / 60); if (minutes < 60) return `${minutes}分钟`; return `${Math.floor(minutes / 60)}小时`; }
export function formatLastToolAge(ms: number): string { const bounded = Math.max(0, ms); if (bounded < 60_000) return `${Math.floor(bounded / 1000)}S前`; if (bounded < 3_600_000) return `${Math.floor(bounded / 60_000)}分钟前`; if (bounded < 86_400_000) return "大于1小时"; return `大于${Math.floor(bounded / 86_400_000)}天`; }
export function lastToolText(tool: LastToolProjection): string { const detail = tool.summary ? `  ${tool.summary}` : ""; return `上次执行工具：${taskKindText[tool.kind]}${detail}`; }
export function taskText(task: TaskProjection | null): string { if (!task || task.state === "idle") return "空闲"; const parts: string[] = [taskKindText[task.kind]]; if (task.summary) parts.push(task.summary); if (task.state === "running" && task.elapsedMs != null) parts.push(formatElapsed(task.elapsedMs)); const state = taskStateText[task.state]; if (state) parts.push(state); return parts.join("  "); }

const lastCommandStatusText = { completed: "成功", failed: "失败", cancelled: "已取消", timed_out: "已超时", lost: "已丢失" } as const;
export function currentActivityText(workflow: CurrentWorkflowProjection | null, command: CurrentCommandProjection | null): string {
  if (command?.state === "cancelling") return "正在取消…";
  if (command?.state === "waiting_input") return "等待输入…";
  if (command?.state === "running") return "运行命令…";
  if (workflow?.state === "running") return "任务执行中…";
  if (workflow?.state === "waiting") return "任务等待继续";
  return "空闲";
}
export function lastCommandText(command: LastCommandProjection): string { return `上次执行：运行命令  结果：${lastCommandStatusText[command.status]}`; }
