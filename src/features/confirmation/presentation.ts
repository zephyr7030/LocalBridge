import { riskText } from "../activity/presentation";

/** 路由说的是"这条命令会怎么落地"，用户需要知道它是自由文本还是结构化操作。 */
const routeText: Record<string, string> = {
  shell: "管理员命令",
  filesystem: "管理员文件操作",
  process: "管理员程序",
};

export function confirmationRouteText(route: string): string {
  return routeText[route] ?? route;
}

export function confirmationRiskText(risk: string[]): string[] {
  return risk.map((item) => riskText[item] ?? item);
}

/** 剩余时间取整到分钟。秒级倒计时会让人觉得被催，而这里不该催。 */
export function confirmationRemainingText(expiresAtMs: number, nowMs: number): string | null {
  const remaining = expiresAtMs - nowMs;
  if (remaining <= 0) return null;
  const minutes = Math.ceil(remaining / 60_000);
  return `${minutes} 分钟后失效`;
}
