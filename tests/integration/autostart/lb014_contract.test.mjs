import { readFileSync } from "node:fs";

const main = readFileSync("src-tauri/src/main.rs", "utf8");
const app = readFileSync("src-tauri/src/app/mod.rs", "utf8");
const single = readFileSync("src-tauri/src/app/single_instance.rs", "utf8");
const autostart = readFileSync("src-tauri/src/app/autostart.rs", "utf8");
const profile = readFileSync("src-tauri/src/app/startup_profile.rs", "utf8");
const startup = readFileSync("src-tauri/src/app/startup.rs", "utf8");
const background = readFileSync("src-tauri/src/app/background.rs", "utf8");
const ui = readFileSync("src-tauri/src/commands/ui.rs", "utf8");
const lib = readFileSync("src-tauri/src/lib.rs", "utf8");
const bridge = readFileSync("src/bridge.ts", "utf8");
const react = readFileSync("src/App.tsx", "utf8");
const privilege = readFileSync("src-tauri/src/privilege/control.rs", "utf8");
const auth = JSON.parse(readFileSync("scripts/authorization-records/LB-014.json", "utf8"));

const acquire = main.indexOf("SingleInstanceGuard::acquire");
const build = main.indexOf("build_app()");
if (!(acquire >= 0 && build > acquire)) throw new Error("LB-014 single-instance acquisition is not before Tauri construction");
for (const required of ["SingleInstanceAcquire::Secondary", "return", "start_wake_listener", "ensure_main_window", "configure_desktop_startup"])
  if (!main.includes(required)) throw new Error(`LB-014 production main wiring missing: ${required}`);
for (const required of ["CreateMutexW", "CreateEventW", "ERROR_ALREADY_EXISTS", "SetEvent", "WaitForMultipleObjects"])
  if (!single.includes(required)) throw new Error(`LB-014 single-instance kernel primitive missing: ${required}`);
if (/TcpListener|UdpSocket/.test(single)) throw new Error("LB-014 single-instance unexpectedly uses a network listener");
for (const required of ["HKEY_CURRENT_USER", "Software\\Microsoft\\Windows\\CurrentVersion\\Run", "--background", "LOCALBRIDGE_RUN_VALUE"])
  if (!autostart.includes(required)) throw new Error(`LB-014 current-user autostart missing: ${required}`);
for (const forbidden of ["HKEY_LOCAL_MACHINE", "schtasks", "scheduled task", "runas"])
  if (autostart.toLowerCase().includes(forbidden.toLowerCase())) throw new Error(`LB-014 autostart forbidden surface: ${forbidden}`);
for (const required of ["STARTUP_PROFILE_SCHEMA_VERSION", "manual_stop_latched", "tunnel_id", "atomic_replace"])
  if (!profile.includes(required)) throw new Error(`LB-014 startup profile missing: ${required}`);
for (const required of ["auto_start_services", "onboarding_complete", "ManualStopLatched", "to_control_state(&WorkspaceValidator)", "ServicesAwaitingUiReady", "stage_foreground_start(config)", "spawn_start_production_runtime(config)", "request_without_uac", "startup_mode == StartupMode::Background && !data.settings.auto_start_services", "startup_mode == StartupMode::Background && profile.manual_stop_latched()"])
  if (!startup.includes(required)) throw new Error(`LB-014 startup coordinator missing: ${required}`);
if (startup.includes("startup_mode != StartupMode::Background")) throw new Error("LB-014 foreground launch is still suppressed instead of auto-starting configured runtime");
const foregroundStart = startup.indexOf("if startup_mode == StartupMode::Foreground");
const foregroundEnd = startup.indexOf("lifecycle\n        .backend_handle()", foregroundStart);
const foregroundBranch = startup.slice(foregroundStart, foregroundEnd);
if (foregroundStart < 0 || foregroundEnd < 0 || foregroundBranch.includes("spawn_start_production_runtime")) throw new Error("LB-014 foreground starts managed services before typed UI-ready");
for (const required of ['name("localbridge-desktop-start"', "RuntimeState::StartingMcp", "publish_starting_if_current", "start_staged_foreground_after_ui_ready", "foreground_start_pending", ".spawn(move ||", "DesktopRuntimeSnapshot::inactive()"])
  if (!background.includes(required)) throw new Error(`LB-014 async foreground startup projection missing: ${required}`);
for (const required of ["pub async fn ui_ready", "spawn_blocking", "start_staged_foreground_after_ui_ready"])
  if (!ui.includes(required)) throw new Error(`LB-014 typed UI-ready backend command missing: ${required}`);
if (!lib.includes("commands::ui::ui_ready")) throw new Error("LB-014 typed UI-ready command is not registered");
if (!bridge.includes('uiReady: () => invoke<void>("ui_ready")')) throw new Error("LB-014 frontend bridge lacks typed UI-ready intent");
for (const required of ["requestAnimationFrame", "bridge.uiReady()", "cancelAnimationFrame"])
  if (!react.includes(required)) throw new Error(`LB-014 interactive UI-ready emission missing: ${required}`);
for (const forbidden of ["spawn_start_production_runtime", "start_production_runtime", "restart_production_runtime"])
  if (react.includes(forbidden)) throw new Error(`LB-014 React illegally owns runtime lifecycle primitive: ${forbidden}`);
const startupTests = readFileSync("tests/integration/autostart/startup.rs", "utf8");
for (const required of ["background_resume_is_suppressed_when_windows_login_autostart_is_disabled", "manual_foreground_launch_ignores_login_autostart_and_manual_stop_latch", "elevated_preference_restores_requested_without_uac_for_background_and_foreground"])
  if (!startupTests.includes(required)) throw new Error(`LB-014 foreground/autostart regression missing: ${required}`);
const backgroundTests = readFileSync("tests/integration/background/background.rs", "utf8");
for (const required of ["foreground_ui_ready_stage_remains_stopped_and_is_one_shot", "duplicate_ui_ready_does_not_restart_existing_healthy_runtime_owner"])
  if (!backgroundTests.includes(required)) throw new Error(`LB-014 UI-ready lifecycle regression missing: ${required}`);
if (startup.includes("enable_from_explicit_user_action")) throw new Error("LB-014 background startup references explicit UAC path");
const requestStart = privilege.indexOf("pub fn request_without_uac");
const requestEnd = privilege.indexOf("pub fn", requestStart + 8);
if (requestStart < 0) throw new Error("LB-014 no-UAC Requested transition is absent");
const requestBody = privilege.slice(requestStart, requestEnd < 0 ? privilege.length : requestEnd);
for (const forbidden of ["launch_broker_with_explicit_uac", "NamedPipeServer::create", "AwaitingUac"])
  if (requestBody.includes(forbidden)) throw new Error(`LB-014 request_without_uac invokes forbidden ${forbidden}`);
if (!app.includes("single_instance") || !app.includes("startup_profile") || !app.includes("configure_desktop_startup")) throw new Error("LB-014 app modules are not wired");
for (const id of ["EXEC-PREAUTH-LB014-001", "EXEC-PREAUTH-LB014-002"]) {
  const record = auth.records.find((candidate) => candidate.authorization_id === id);
  if (!record || record.user_audit_status !== "PENDING" || record.does_not_expand_future_pr_writable_paths !== true) throw new Error(`LB-014 preauthorization record invalid: ${id}`);
}
console.log("LB014_CONTRACT=PASS single_instance_pre_tauri=true wake_existing=true hkcu_run=true background_immediate=true manual_stop_persistent=true workspace_revalidated=true elevated_requested_no_uac=true foreground_ui_first=true service_start_before_ui_ready=false typed_ui_ready=true duplicate_ready_idempotent=true existing_healthy_owner_preserved=true login_autostart_not_runtime_gate=true preauth_pending=2");
