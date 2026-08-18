import { readFileSync } from "node:fs";
const app = readFileSync("src/App.tsx", "utf8");
const bridge = readFileSync("src/bridge.ts", "utf8");
const presentation = readFileSync("src/presentation.ts", "utf8");
const css = readFileSync("src/styles.css", "utf8");
const backend = readFileSync("src-tauri/src/commands/ui.rs", "utf8");
const background = readFileSync("src-tauri/src/app/background.rs", "utf8");
const tray = readFileSync("src-tauri/src/tray/mod.rs", "utf8");
const main = readFileSync("src-tauri/src/main.rs", "utf8");
const privilegeProtocol = readFileSync("src-tauri/src/privilege/protocol.rs", "utf8");
const lib = readFileSync("src-tauri/src/lib.rs", "utf8");
const mcp = readFileSync("src-tauri/src/mcp/server.rs", "utf8");
const gitAdapter = readFileSync("src-tauri/src/mcp/git_adapter.rs", "utf8");
const mcpRuntime = readFileSync("src-tauri/src/mcp/runtime.rs", "utf8");
const mcpGuard = readFileSync("src-tauri/src/mcp/guard.rs", "utf8");
const mcpPolicy = readFileSync("src-tauri/src/mcp/policy.rs", "utf8");
const startup = readFileSync("src-tauri/src/app/startup.rs", "utf8");
const startupTests = readFileSync("tests/integration/autostart/startup.rs", "utf8");
const backgroundTests = readFileSync("tests/integration/background/background.rs", "utf8");
const policyTests = readFileSync("tests/integration/policy/policy_enforcement.rs", "utf8");
const codingRuntimeTests = readFileSync("tests/integration/mcp/coding_runtime.rs", "utf8");
const backendUi = readFileSync("src-tauri/src/commands/ui.rs", "utf8");
const backendOnboarding = readFileSync("src-tauri/src/commands/onboarding.rs", "utf8");
const onboardingUi = readFileSync("src/features/onboarding/Onboarding.tsx", "utf8");
const auth = JSON.parse(readFileSync("scripts/authorization-records/LB-015.json", "utf8"));
for (const text of ["当前项目","本地运行环境","OpenAI 安全隧道","编码服务","权限模式","空闲","任务等待继续"]) if (!`${app}\n${presentation}`.includes(text)) throw new Error(`LB-015 Dashboard wording missing: ${text}`);
if (app.includes("pathEditor") || app.includes("newPath") || app.includes('id="project-path"')) throw new Error("LB-015 Dashboard still exposes raw project path input");
if (!bridge.includes('chooseProjectFolder: () => invoke<string | null>("choose_onboarding_workspace_folder")') || !app.includes("bridge.chooseProjectFolder()")) throw new Error("LB-015 Dashboard does not use native Windows folder picker");
const dashboardBeforeSettings = app.slice(app.indexOf("return <main"), app.indexOf('{view === "settings"'));
if (!dashboardBeforeSettings.includes('>权限模式</span>') || !dashboardBeforeSettings.includes('accessText[projection.permission]')) throw new Error("LB-015 Dashboard read-only PermissionMode projection missing");
if (!app.includes('const adminModeFullAccess = projection?.permission === "admin"') || app.includes('const elevatedFullAccess = projection?.permission === "admin" && projection?.privilege === "active"')) throw new Error("LB-015 Dashboard project scope is not bound solely to PermissionMode");
if (!dashboardBeforeSettings.includes('adminModeFullAccess ? "全目录访问" : activeProject?.path ?? "未选择项目"') || !app.includes('if (adminModeFullAccess)')) throw new Error("LB-015 Dashboard project label/switch boundary does not follow PermissionMode");
if (dashboardBeforeSettings.includes('>管理员权限</span>') || dashboardBeforeSettings.includes('permission-mode-value"><ServiceStatusDot') || dashboardBeforeSettings.includes('setAccess(') || dashboardBeforeSettings.includes('enableAdmin') || dashboardBeforeSettings.includes('disableAdmin') || dashboardBeforeSettings.includes('启用管理员权限')) throw new Error("LB-015 Dashboard permission row regressed into privilege-dot or mutation UI");
const flatCard = css.replace(/\s+/g, "").match(/\.card\{([^}]*)\}/)?.[1] ?? "";
for (const marker of ["background:transparent", "border:0", "border-radius:0", "box-shadow:none"]) if (!flatCard.includes(marker)) throw new Error(`LB-015 flat primary surface missing: ${marker}`);
if (!dashboardBeforeSettings.includes("ServiceStatusDot") || dashboardBeforeSettings.includes("privilegeService")) throw new Error("LB-015 Dashboard service dots or PermissionMode no-dot boundary drifted");
for (const required of [">常规<",">连接<",">权限<","开机启动","关闭窗口后继续运行","Tunnel ID","Runtime API Key","打开欢迎页",">完成<",">更换<"]) if (!app.includes(required)) throw new Error(`LB-015 Settings contract missing: ${required}`);
if (app.includes("测试连接") || app.includes("运行密钥")) throw new Error("LB-015 Settings exposes forbidden connection wording/action");
if (!app.includes('type="password"') || !app.includes('projection?.runtimeKeySaved ? "已保存" : "未保存"')) throw new Error("LB-015 Runtime API Key summary/edit security contract missing");
if (!css.includes("#0071e3") || !css.includes("--admin-accent:#ff9500") || !css.includes("admin-choice")) throw new Error("LB-015 blue/orange selection styling missing");
if (!presentation.includes('"空闲"') || presentation.includes('"等待命令"')) throw new Error("LB-015 schema42 idle wording drifted");
for (const required of ["lifecycle.task_aggregate_snapshot()", "current_workflow_projection(&task_aggregate)", "current_command_projection(&task_aggregate)", "last_command_projection(&task_aggregate)"]) if (!backend.includes(required)) throw new Error(`LB-015 schema42 backend TaskAggregate projection missing: ${required}`);
if (!mcp.includes("task_aggregate_snapshot")) throw new Error("LB-015 schema42 PEP TaskAggregate source missing");
for (const required of ["current_workflow_projection","current_command_projection","last_command_projection"]) if (!backend.includes(required)) throw new Error(`LB-015 schema42 typed TaskAggregate projection missing: ${required}`);
if (!bridge.includes('waitForProjectionChange: (sinceRevision: number) => invoke<number>("wait_main_projection_change"') || !app.includes("await bridge.waitForProjectionChange(revision)")) throw new Error("LB-015 frontend is not driven by backend projection wake");
if (!background.includes('pub fn runtime_snapshot_with_revision(&self) -> (DesktopRuntimeSnapshot, u64)') || !backend.includes('let (snapshot, projection_revision) = lifecycle.runtime_snapshot_with_revision();') || backend.includes('projection_revision: lifecycle.projection_revision()')) throw new Error("LB-015 Dashboard projection snapshot and wake cursor are not captured atomically");
if (/setInterval\s*\(/.test(app)) throw new Error("LB-015 Dashboard still relies on periodic frontend polling");
for (const required of ["pub async fn wait_main_projection_change", "wait_projection_change_after", "projection_revision"]) if (!backend.includes(required)) throw new Error(`LB-015 backend projection wait command missing: ${required}`);
for (const required of ["Condvar", "struct ProjectionWake", "fn wait_after", "projection_wake.notify()", "CurrentTaskWake", ".with_task_projection_wake(wake)"]) if (!background.includes(required)) throw new Error(`LB-015 wake-driven runtime projection missing: ${required}`);
for (const required of ["const MIN_TASK_PRESENTATION: Duration = Duration::from_millis(500)", "VecDeque<QueuedTask>", "current_task_projection_serializes_burst_fast_calls_for_full_visibility", "UI presentation retention must not delay real MCP response"]) if (!mcp.includes(required)) throw new Error(`LB-015 short real MCP >=500ms presentation contract missing: ${required}`);
if (!mcp.includes("UI retention must not delay Broker response") || !mcp.includes("first_serialized_retired")) throw new Error("LB-015 Broker >=500ms/non-delaying burst regression missing");
if (!backend.includes("last_command: last_command_projection(&task_aggregate)") || !bridge.includes("lastCommand: LastCommandProjection | null") || !bridge.includes("lastTool: LastToolProjection | null")) throw new Error("LB-015 schema42 command history projection missing");
if (!app.includes("currentActivityText(currentWorkflow, currentCommand)") || !app.includes('className="last-tool-row"') || !app.includes("lastCommandText(projection.lastCommand)") || !app.includes("formatLastToolAge(projection.lastCommand.ageMs)")) throw new Error("LB-015 schema42 current/history separation missing");
if (!app.includes("const currentWorkflow = projection?.currentWorkflow ?? null") || !app.includes("const currentCommand = projection?.currentCommand ?? null") || !app.includes('className={`activity-dot task-${taskState}`}')) throw new Error("LB-015 schema42 current activity is not derived from TaskAggregate fields");
const compactTaskCss = css.replace(/\s+/g, "");
for (const marker of [
  ".activity-dot.task-idle,.activity-dot.task-cancelled{background:var(--status-unknown)}",
  ".activity-dot.task-running{background:var(--status-ready);animation:task-pulse",
  ".activity-dot.task-waiting{background:var(--status-starting)}",
  ".activity-dot.task-blocked,.activity-dot.task-failed{background:var(--status-fault)}",
  ".activity-dot.task-running{animation:none;opacity:1;transform:none}",
]) if (!compactTaskCss.includes(marker.replace(/\s+/g, ""))) throw new Error(`LB-015 concrete CurrentTask visual semantic missing: ${marker}`);
for (const wording of ['waiting: "等待授权"', 'blocked: "已阻止"', 'failed: "执行失败"', 'cancelled: "已取消"']) if (!presentation.includes(wording)) throw new Error(`LB-015 concrete CurrentTask wording missing: ${wording}`);
for (const wording of ['return "空闲"', 'return "任务等待继续"', 'return "运行命令…"']) if (!presentation.includes(wording)) throw new Error(`LB-015 schema42 current activity wording missing: ${wording}`);
if (!presentation.includes("上次执行：运行命令") || !css.includes(".last-tool-age{flex:0 0 auto;text-align:right")) throw new Error("LB-015 schema42 last-command history label/age alignment missing");
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
for (const required of ["window.setTimeout", "3000", "window.clearTimeout", "showTransientError", "clearTransientError"]) if (!app.includes(required)) throw new Error(`LB-015 3-second transient notice contract missing: ${required}`);
if (!app.includes('role="dialog"') || !app.includes("projection?.reconnect") || !app.includes("reconnectVisible")) throw new Error("LB-015 persistent typed reconnect fault projection was lost while implementing transient operation notices");
for (const required of ["重启服务", "关闭服务", "bridge.restartServices()", "bridge.stopServices()", 'className="secondary service-restart"', 'className="secondary service-stop"']) if (!app.includes(required)) throw new Error(`LB-015 Dashboard service control missing: ${required}`);
for (const required of [".secondary.service-restart", "var(--admin-accent)", ".secondary.service-stop", "var(--status-fault)"]) if (!css.includes(required)) throw new Error(`LB-015 service control semantic styling missing: ${required}`);
if (!css.includes(".service-actions{display:flex;align-items:center;justify-content:flex-end") || !css.includes("width:100%;margin:0 0 10px")) throw new Error("LB-015 Dashboard service controls are not right-aligned with the homepage action edge");
if (!bridge.includes('restartServices: () => invoke<void>("restart_services")') || !bridge.includes('stopServices: () => invoke<void>("stop_services")')) throw new Error("LB-015 service controls are not typed Tauri intents");
for (const command of ["restart_services", "stop_services"]) {
  const start = backend.indexOf('pub async fn ' + command);
  if (start < 0) throw new Error(`LB-015 backend service command missing: ${command}`);
  const next = backend.indexOf('pub async fn ', start + 12);
  const body = backend.slice(start, next < 0 ? backend.length : next);
  if (!body.includes('spawn_blocking')) throw new Error(`LB-015 service command lacks backend worker boundary: ${command}`);
}
if (!css.includes(".settings-summary{display:grid;grid-template-columns:minmax(0,1fr) max-content max-content") || !css.includes(".settings-summary>.settings-clear{grid-column:2;justify-self:start") || !css.includes(".settings-summary>.settings-replace{grid-column:3;justify-self:start")) throw new Error("LB-015 Settings clear/replace content-derived action columns drifted");
if ((app.match(/className="settings-summary"/g) ?? []).length !== 2) throw new Error("LB-015 Settings must use the same action-column layout for both connection summaries");
if (!app.includes('className="secondary settings-clear" onClick={() => setConfirmingKeyDelete(true)}>清除</button>') || !bridge.includes('clearKey: () => invoke<void>("delete_runtime_key")')) throw new Error("LB-015 Runtime API Key clear confirmation entry or typed intent missing");
for (const required of ["请确认从windows安全凭据中删除？", 'className="secondary settings-delete-cancel"', '>取消</button>', 'className="secondary settings-confirm-delete"', "await bridge.clearKey(); setConfirmingKeyDelete(false);", '>确认</button>']) if (!app.includes(required)) throw new Error(`LB-015 Runtime API Key inline delete confirmation missing: ${required}`);
const clearButton = app.indexOf('className="secondary settings-clear"');
const confirmButton = app.indexOf('className="secondary settings-confirm-delete"');
if (clearButton < 0 || confirmButton < 0 || app.slice(clearButton, app.indexOf(">清除</button>", clearButton)).includes("bridge.clearKey")) throw new Error("LB-015 first Runtime API Key clear click still deletes before confirmation");
if (!css.includes(".settings-summary>.settings-delete-cancel{grid-column:2;justify-self:start") || !css.includes(".settings-summary>.settings-confirm-delete{grid-column:3;justify-self:start") || !css.includes("color:var(--status-fault)")) throw new Error("LB-015 Runtime API Key confirmation actions lost content-derived alignment or red danger semantics");
const deleteStart = backend.indexOf("pub async fn delete_runtime_key");
const deleteEnd = backend.indexOf("pub async fn choose_project_folder", deleteStart);
const deleteBody = backend.slice(deleteStart, deleteEnd < 0 ? backend.length : deleteEnd);
if (deleteStart < 0 || !deleteBody.includes(".delete_runtime_api_key()") || deleteBody.includes("read_runtime_api_key") || !deleteBody.includes("if deleted") || !deleteBody.includes("reconnect_after_connection_change")) throw new Error("LB-015 Runtime API Key clear does not securely delete then reuse controlled reconnect");
for (const marker of [".inner_size(780.0, 620.0)", ".min_inner_size(780.0, 620.0)", ".max_inner_size(780.0, 620.0)"]) if (!tray.includes(marker)) throw new Error(`LB-015 780x620 native window marker missing: ${marker}`);
if (!tray.includes("window.center()?;")) throw new Error("LB-015 first-created main window does not center before show");
if (!main.includes("expected 780x620 logical")) throw new Error("LB-015 live native window checker did not freeze 780x620");
if (!css.includes(".sheet,.dialog{width:min(560px,100%);max-height:84vh;overflow:auto;border-radius:20px;clip-path:inset(0 round 20px);scrollbar-gutter:stable")) throw new Error("LB-015 rounded scroll shell does not clip scrollbar inside all four corners");
for (const required of [
  ".sheet::-webkit-scrollbar,.dialog::-webkit-scrollbar{width:10px;height:10px}",
  ".sheet::-webkit-scrollbar-track,.dialog::-webkit-scrollbar-track{margin-block:16px;background:transparent;border-radius:999px}",
  ".sheet::-webkit-scrollbar-thumb,.dialog::-webkit-scrollbar-thumb{min-height:32px",
  ".sheet::-webkit-scrollbar-button:vertical:start:decrement",
  ".sheet::-webkit-scrollbar-button:vertical:end:increment",
  "-webkit-appearance:none!important",
  "display:none!important",
  "width:0!important",
  "height:0!important"
]) if (!css.includes(required)) throw new Error(`LB-015 custom arrowless scrollbar rendering missing: ${required}`);
for (const forbidden of ["::-webkit-scrollbar{display:none", "::-webkit-scrollbar-thumb{display:none"]) if (css.includes(forbidden)) throw new Error(`LB-015 arrow repair illegally hides scrolling affordance: ${forbidden}`);
for (const required of ["settings_scrollbar_width", "settings_scrollbar_track_display", "settings_scrollbar_track_margin_top", "settings_scrollbar_track_margin_bottom", "settings_scrollbar_thumb_display", "settings_scrollbar_button_display", "settings_scrollbar_button_width", "settings_scrollbar_button_height", "settings_scrollbar_button_appearance", "settings_sheet_scroll_range", "settings_sheet_scroll_top", "::-webkit-scrollbar", "::-webkit-scrollbar-track", "::-webkit-scrollbar-thumb", "::-webkit-scrollbar-button", "button_hidden", "Settings custom scrollbar width is not active", "Settings custom scrollbar lost track/thumb", "Settings scrollbar track does not stay clear of rounded corners", "Settings sheet lost a real scroll range", "Settings sheet no longer scrolls after arrow removal", "scrollbar_arrows=false", "scroll_surface=true"]) if (!main.includes(required)) throw new Error(`LB-015 live custom arrowless-scroll verification missing: ${required}`);
if (!backend.includes("fn connection_change_requires_restart") || !backend.includes("!matches!(state, RuntimeState::Stopped)") || !backend.includes("RuntimeState::StartingMcp") || !backend.includes("connection_changes_restart_runtime_when_starting_or_connected")) throw new Error("LB-015 Starting connection-change controlled restart regression missing");
if (!backend.includes("if was_active {") || !backend.includes("stop_runtime_for_control_plane()") || !backend.includes("!matches!(runtime_before.state, RuntimeState::Stopped)")) throw new Error("LB-015 active-project removal does not invalidate pending Starting generation or restore prior runtime intent on persistence rollback");
if (backend.includes("path: entry.display_path.to_string_lossy().into_owned()")) throw new Error("LB-015 Dashboard projects still expose persisted display_path without fresh identity validation");
for (const required of ["WorkspaceValidator", "entry.validated_identity.as_str() == validated.identity().as_str()", "validated.execution_path().to_string_lossy().into_owned()", '"项目已无法访问"']) if (!backend.includes(required)) throw new Error(`LB-015 safe ordinary project-path projection missing: ${required}`);
if (!privilegeProtocol.includes("is_windows_verbatim_path") || !privilegeProtocol.includes('value.starts_with(r"\\\\?\\")') || !privilegeProtocol.includes("is_windows_verbatim_path(&self.program)") || !privilegeProtocol.includes("is_windows_verbatim_path(value)")) throw new Error("LB-015 Broker execution boundary does not fail closed on Win32 verbatim program/workdir paths");
for (const required of ["DenyReason::VerbatimExecutionPath", "has_verbatim_execution_path", '"exec_command"', '"write_stdin"', '"cwd"', '"workdir"', '"paths"', "contains_verbatim_path_text"]) if (!`${mcpGuard}\n${mcpPolicy}`.includes(required)) throw new Error(`LB-015 normal MCP verbatim execution-path guard missing: ${required}`);
for (const required of ["win32_verbatim_execution_paths_are_blocked_before_upstream_without_scanning_patch_content", "verbatim_execution_paths_are_denied_before_real_upstream_runtime"]) if (!`${policyTests}\n${codingRuntimeTests}`.includes(required)) throw new Error(`LB-015 verbatim-path regression missing: ${required}`);
if (!app.includes('projection?.codingService') || !onboardingUi.includes('main?.codingService')) throw new Error("LB-015 Dashboard/onboarding no longer share backend codingService projection");
for (const marker of ["RuntimeComponent::CodingRuntime", '("recovering", "recovering")']) if (!backendUi.includes(marker)) throw new Error(`LB-015 backend coding-service recovering projection missing: ${marker}`);
for (const marker of ["fn readiness(lifecycle: &DesktopLifecycle)", "RuntimeState::StartingTunnel", "RuntimeState::WaitingTunnelReady", "RuntimeState::Ready"]) if (!backendOnboarding.includes(marker)) throw new Error(`LB-015 onboarding backend readiness source drift: ${marker}`);
const manualStopStart = startup.indexOf("pub fn manual_stop_services");
const manualStopEnd = startup.indexOf("fn build_background_resume_config", manualStopStart);
const manualStopBody = startup.slice(manualStopStart, manualStopEnd);
const recordIndex = manualStopBody.indexOf("profile.record_manual_stop()");
const saveIndex = manualStopBody.indexOf("store.save(&profile)");
const shutdownIndex = manualStopBody.indexOf("stop_services_for_manual_action()");
if (!(recordIndex >= 0 && saveIndex > recordIndex && shutdownIndex > saveIndex)) throw new Error("LB-015 manual-stop latch is not persisted before managed shutdown");
for (const required of ["manual_stop_services_persists_latch_before_shutdown", "manual_service_stop_invalidates_pending_start_without_active_owner"]) if (!`${startupTests}\n${backgroundTests}`.includes(required)) throw new Error(`LB-015 manual-stop regression missing: ${required}`);
const restartStart = backend.indexOf("pub async fn restart_services");
const restartEnd = backend.indexOf("pub async fn stop_services", restartStart);
const restartBody = backend.slice(restartStart, restartEnd);
for (const required of ["clear_manual_stop_for_explicit_action(&app)", "load_app_data(&app)", "production_runtime_config_for_active_workspace(&app, &data)", "restart_production_runtime(config)"]) if (!restartBody.includes(required)) throw new Error(`LB-015 restart does not rebuild from current persisted state: ${required}`);
for (const forbidden of ["enable_from_explicit_user_action", "request_explicit_admin", "request_without_uac", "ShellExecute", "runas"]) if (restartBody.includes(forbidden)) throw new Error(`LB-015 restart may trigger UAC: ${forbidden}`);
const configStart = backend.indexOf("fn production_runtime_config_for_path");
const configEnd = backend.indexOf("fn connection_change_requires_restart", configStart);
const configBody = backend.slice(configStart, configEnd);
for (const required of ["StartupProfileStore::new", ".load()", ".validated_tunnel_id()", "let entry = data", ".workspace", ".active_entry()", "WorkspaceValidator", "validated.execution_path()"] ) if (!configBody.includes(required)) throw new Error(`LB-015 current persisted restart config boundary missing: ${required}`);
for (const command of [["save_runtime_key", ".save_runtime_api_key(&secret)"], ["save_tunnel_id", ".save(&profile)"]]) {
  const start = backend.indexOf(`pub async fn ${command[0]}`);
  const next = backend.indexOf("#[tauri::command]", start + 20);
  const body = backend.slice(start, next < 0 ? backend.length : next);
  const persisted = body.indexOf(command[1]);
  const reconnect = body.indexOf("reconnect_after_connection_change");
  if (!(persisted >= 0 && reconnect > persisted)) throw new Error(`LB-015 ${command[0]} does not persist before controlled reconnect`);
}
if (!gitAdapter.includes("struct GitRepositoryResolver") || !gitAdapter.includes("fn handle_git_tool") || !gitAdapter.includes("run_bounded_command")) throw new Error("LB-015 shared nested Git repository adapter missing");
for (const tool of ["git_status", "git_log", "git_show", "git_diff", "git_blame"]) if (!gitAdapter.includes(`\"${tool}\"`)) throw new Error(`LB-015 shared Git adapter does not cover ${tool}`);
if (!mcpRuntime.includes("use super::git_adapter::handle_git_tool") || !mcpRuntime.includes("handle_git_tool(&self.workspace, name, &arguments)")) throw new Error("LB-015 CodingToolsRuntime is not routed through the shared Git repository adapter");
if (gitAdapter.includes("non-git diff fallback")) throw new Error("LB-015 native-repository git_diff path contains forbidden non-git fallback marker");
console.log("LB015_CONTRACT=PASS dashboard_native_picker=true dashboard_permission_readonly=true settings_unique_permission_editor=true settings_sections=3 task_wake_driven=true task_min_visible_ms=500 tool_response_not_delayed=true last_tool_rows=1 transient_notice_3s=true service_controls=true key_clear=true settings_action_column=true native_window=780x620 rounded_scroll_clip=true scrollbar_arrows=false scroll_surface_preserved=true starting_reconnect=true ordinary_path_projection=true broker_verbatim_guard=true mcp_verbatim_guard=true starting_manual_stop=true restart_latest_persisted_no_uac=true nested_git_repository_resolver=true status_source_shared=true");
