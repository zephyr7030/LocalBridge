import type { AccessCode, PrivilegeCode, ServiceCode, TaskProjection } from "./bridge";
export const uiText = { dashboard: "主控界面", settings: "设置", diagnostics: "诊断" } as const;
export const accessText: Record<AccessCode, string> = { edit: "编辑模式", full: "完整模式", admin: "管理员模式" };
export const privilegeText: Record<PrivilegeCode, string> = { off: "未启用", requested: "等待授权", awaiting: "等待系统授权", active: "已启用", fault: "故障" };
export const serviceText: Record<ServiceCode, string> = { off: "未启动", starting: "正在启动", online: "已连接", recovering: "正在恢复", fault: "连接失败" };
const taskKindText = { read: "读取文件", search: "搜索代码", modify: "修改文件", command: "运行命令", git: "版本操作", build: "构建项目", test: "运行测试", admin: "管理员操作", other: "处理任务" } as const;
const taskStateText = { idle: "空闲", running: "", waiting: "等待授权", blocked: "已阻止", failed: "失败", cancelled: "已取消" } as const;
export function taskText(task: TaskProjection | null): string { if (!task) return "空闲"; const parts: string[] = [taskKindText[task.kind]]; if (task.summary) parts.push(task.summary); const state = taskStateText[task.state]; if (state) parts.push(state); return parts.join("  "); }
