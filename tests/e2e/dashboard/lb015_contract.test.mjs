import { readFileSync } from "node:fs";
const app = readFileSync("src/App.tsx", "utf8");
const bridge = readFileSync("src/bridge.ts", "utf8");
const presentation = readFileSync("src/presentation.ts", "utf8");
const css = readFileSync("src/styles.css", "utf8");
const backend = readFileSync("src-tauri/src/commands/ui.rs", "utf8");
const lib = readFileSync("src-tauri/src/lib.rs", "utf8");
const mcp = readFileSync("src-tauri/src/mcp/server.rs", "utf8");
const auth = JSON.parse(readFileSync("scripts/authorization-records/LB-015.json", "utf8"));
for (const text of ["当前项目","本地运行环境","OpenAI 安全隧道","编码服务","管理员权限","等待命令"]) if (!`${app}\n${presentation}`.includes(text)) throw new Error(`LB-015 Dashboard wording missing: ${text}`);
if (app.includes("pathEditor") || app.includes("newPath") || app.includes('id="project-path"')) throw new Error("LB-015 Dashboard still exposes raw project path input");
if (!bridge.includes('chooseProjectFolder: () => invoke<string | null>("choose_onboarding_workspace_folder")') || !app.includes("bridge.chooseProjectFolder()")) throw new Error("LB-015 Dashboard does not use native Windows folder picker");
const dashboardBeforeSettings = app.slice(app.indexOf("return <main"), app.indexOf('{view === "settings"'));
for (const forbidden of ["权限模式","编辑模式","完整模式","管理员模式","setAccess(","enableAdmin","disableAdmin","启用管理员权限"]) if (dashboardBeforeSettings.includes(forbidden)) throw new Error(`LB-015 Dashboard permission mutation surface remains: ${forbidden}`);
if (!dashboardBeforeSettings.includes("ServiceStatusDot") || !dashboardBeforeSettings.includes("privilegeService")) throw new Error("LB-015 Dashboard lacks shared typed status dots/read-only privilege state");
for (const required of [">常规<",">连接<",">权限<","开机启动","关闭窗口后继续运行","Tunnel ID","Runtime API Key","打开欢迎页",">完成<",">更换<"]) if (!app.includes(required)) throw new Error(`LB-015 Settings contract missing: ${required}`);
if (app.includes("测试连接") || app.includes("运行密钥")) throw new Error("LB-015 Settings exposes forbidden connection wording/action");
if (!app.includes('type="password"') || !app.includes('projection?.runtimeKeySaved ? "已保存" : "未保存"')) throw new Error("LB-015 Runtime API Key summary/edit security contract missing");
if (!css.includes("#0071e3") || !css.includes("admin-choice")) throw new Error("LB-015 blue/amber selection styling missing");
if (!presentation.includes('return "等待命令"') || presentation.includes('return "空闲"')) throw new Error("LB-015 no-task wording drifted");
if (!backend.includes("current_task: task_projection(&snapshot.current_task)")) throw new Error("LB-015 Dashboard task does not originate in backend typed projection");
for (const required of ["CurrentTaskStatus::Active", "CurrentTaskStatus::Idle", "current_task.project(status)"]) if (!mcp.includes(required)) throw new Error(`LB-015 production MCP CurrentTask plumbing missing: ${required}`);
if (bridge.includes("enable_admin") || bridge.includes("disable_admin") || lib.includes("commands::ui::enable_admin") || lib.includes("commands::ui::disable_admin")) throw new Error("LB-015 obsolete standalone administrator command remains registered");
const permissionStart = backend.indexOf("pub async fn set_permission_mode");
const permissionEnd = backend.indexOf("fn request_explicit_admin", permissionStart);
const permissionBody = backend.slice(permissionStart, permissionEnd);
if (permissionStart < 0 || !permissionBody.includes("enable_from_explicit_user_action") && !backend.includes("enable_from_explicit_user_action") || !permissionBody.includes("requested == PermissionMode::Elevated") || !permissionBody.includes("previous == PermissionMode::Elevated") || !permissionBody.includes(".disable()")) throw new Error("LB-015 Settings permission selection does not own UAC/disable semantics");
if (!/previous != PermissionMode::Elevated[\s\S]*?\.privilege\(\)[\s\S]*?\.disable\(\)/.test(permissionBody)) throw new Error("LB-015 failed first-time elevation does not restore a valid non-elevated privilege state");
for (const id of ["EXEC-PREAUTH-LB015-006"]) { const r=auth.records.find(x=>x.authorization_id===id); if(!r || r.user_audit_status!=="PENDING") throw new Error(`LB-015 authorization history invalid: ${id}`); }
if (!app.includes('projectPickerOpen') || !app.includes('if (item.active)') || !app.includes('bridge.removeProject(item.id)')) throw new Error("LB-015 project picker does not expose direct non-active metadata removal");
if (!app.includes('不会删除项目文件')) throw new Error("LB-015 active-project removal confirmation lost no-file-delete guarantee");
for (const command of ["get_main_projection", "retry_connection", "add_project", "select_project", "remove_project"]) {
  const start = backend.indexOf('pub async fn ' + command);
  if (start < 0) throw new Error('LB-015 blocking Tauri command remains: ' + command);
  const next = backend.indexOf('pub async fn ', start + 12);
  const body = backend.slice(start, next < 0 ? backend.length : next);
  if (!body.includes('spawn_blocking')) throw new Error('LB-015 command lacks backend worker boundary: ' + command);
}
console.log("LB015_CONTRACT=PASS dashboard_native_picker=true dashboard_permission_readonly=true settings_unique_permission_editor=true settings_sections=3 runtime_api_key_exact=true task_waiting_command=true current_task_production_plumbing=true status_source_shared=true");
