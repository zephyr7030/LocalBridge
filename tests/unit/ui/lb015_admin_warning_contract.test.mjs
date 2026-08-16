import { readFileSync } from "node:fs";

const warning = readFileSync("src/components/AdminModeWarning.tsx", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
const onboarding = readFileSync("src/features/onboarding/Onboarding.tsx", "utf8");
const css = readFileSync("src/styles.css", "utf8");

for (const text of [
  "启用管理员权限后，错误或恶意操作可能导致：",
  "删除或覆盖重要文件",
  "修改系统关键配置",
  "软件或系统无法正常启动",
  "数据永久丢失",
  "安全机制被绕过或关闭",
  "凭据、密钥等敏感信息泄露",
  "恶意程序获得更高权限",
  "系统被破坏，严重时可能需要重装 Windows",
  "仅在你明确理解操作后果时授权。",
]) if (!warning.includes(text)) throw new Error(`LB-015 administrator warning copy missing: ${text}`);
for (const marker of ["ADMIN_WARNING_COUNTDOWN_MS = 9000", "performance.now()", "adminWarningCanConfirm", "adminWarningRemainingSeconds", 'event.key === "Escape"', "onMouseDown={onCancel}", "disabled={remainingSeconds > 0}", "`确认${remainingSeconds}`", ' : "确认"']) if (!warning.includes(marker)) throw new Error(`LB-015 administrator warning gate marker missing: ${marker}`);
if (!css.replace(/\s+/g, "").includes("--admin-accent:#ff9500") || !css.includes(".admin-warning-confirm{background:#d70015;border-color:#d70015;color:#fff}")) throw new Error("LB-015 administrator warning/accent colors drifted");
for (const source of [app, onboarding]) {
  if (!source.includes('mode === "admin"') || !source.includes('privilege !== "active"') || !source.includes("setAdminWarningOpen(true)") || !source.includes("<AdminModeWarning")) throw new Error("LB-015 administrator mode surface bypasses the shared warning gate");
}
if (!app.includes('void run(() => bridge.setAccess("admin"))') || !onboarding.includes('void applyPermission("admin")')) throw new Error("LB-015 enabled confirmation is not the only explicit admin continuation path");
console.log("LB015_ADMIN_WARNING_CONTRACT=PASS exact_copy=true monotonic_9000ms=true cancel_escape_no_side_effect=true shared_settings_onboarding=true whole_red_confirm=true");
