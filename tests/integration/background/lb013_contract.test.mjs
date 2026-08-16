import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const main = readFileSync("src-tauri/src/main.rs", "utf8");
const tray = readFileSync("src-tauri/src/tray/mod.rs", "utf8");
const background = readFileSync("src-tauri/src/app/background.rs", "utf8");
const startup = readFileSync("src-tauri/src/app/startup.rs", "utf8");
const settingsModel = readFileSync("src-tauri/src/settings/model.rs", "utf8");
const migration = readFileSync("src-tauri/src/settings/migration.rs", "utf8");
const migrationTest = readFileSync("tests/migrations/migration_matrix.rs", "utf8");
const privilege = readFileSync("src-tauri/src/privilege/control.rs", "utf8");
const orchestrator = readFileSync("src-tauri/src/runtime/orchestrator.rs", "utf8");
const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const auth = JSON.parse(readFileSync("scripts/authorization-records/LB-013.json", "utf8"));
const trayDeriver = readFileSync("scripts/icons/derive-tray-icon.ps1", "utf8");
const normalized = (value) => value.replace(/\s+/g, " ");

if ((config.app?.windows ?? []).length !== 0) throw new Error("LB-013 background startup still has a static Tauri main window");
if (!main.includes('windows_subsystem = "windows"')) throw new Error("LB-013 LocalBridge app binary can still expose a console window");
for (const required of ["StartupMode::from_args", "creates_main_window_at_startup", "ensure_main_window", "install_tray", "CloseRequested", "prevent_close", "close_window_continue_running", "backend_handle", "spawn_shutdown_then", ".hide()"])
  if (!main.includes(required)) throw new Error(`LB-013 entry lifecycle missing: ${required}`);
const closeHandlerStart = main.indexOf("fn handle_main_window_event");
const closeHandlerEnd = main.indexOf("#[cfg(debug_assertions)]", closeHandlerStart);
const closeHandler = main.slice(closeHandlerStart, closeHandlerEnd);
if (!closeHandler.includes("if lifecycle.close_window_continue_running()") || !closeHandler.includes("spawn_shutdown_then(move |_| app.exit(0))"))
  throw new Error("LB-013 CloseRequested does not implement cached hide-vs-orderly-exit policy");
if (closeHandler.includes("SettingsStore::new") || closeHandler.includes("lifecycle.shutdown()"))
  throw new Error("LB-013 CloseRequested performs blocking settings/lifecycle work on the UI event path");
const modeGuard = main.indexOf("creates_main_window_at_startup");
const createCall = main.indexOf("ensure_main_window", modeGuard);
if (!(modeGuard >= 0 && createCall > modeGuard)) throw new Error("LB-013 foreground window creation is not startup-mode gated");
for (const required of [
  ".visible(false)",
  ".inner_size(780.0, 620.0)",
  ".min_inner_size(780.0, 620.0)",
  ".max_inner_size(780.0, 620.0)",
  ".resizable(false)",
  ".maximizable(false)",
  ".decorations(false)",
  "sync_main_webview_to_client(app, window.inner_size()?)?",
]) if (!tray.includes(required)) throw new Error(`LB-013 fixed borderless main-window contract missing: ${required}`);
for (const forbidden of [
  "MAIN_WINDOW_PHYSICAL_WIDTH",
  "MAIN_WINDOW_PHYSICAL_HEIGHT",
  "window.set_min_size(Some(physical))?",
  "window.set_max_size(Some(physical))?",
  "window.set_size(physical)?",
  "window.set_zoom(1.0 / scale)?",
  "enforce_main_window_metrics",
  ".min_inner_size(720.0, 500.0)",
  ".resizable(true)",
  ".maximizable(true)",
]) if (tray.includes(forbidden)) throw new Error(`LB-013 stale native window behavior remains: ${forbidden}`);
for (const required of ["WindowEvent::ScaleFactorChanged", "sync_main_webview_to_client(window.app_handle(), client_size)"])
  if (!main.includes(required)) throw new Error(`LB-013 DPI-change WebView client sync missing: ${required}`);
for (const forbidden of ["enforce_main_window_metrics(window.app_handle())", "set_zoom(1.0 / scale)"])
  if (main.includes(forbidden)) throw new Error(`LB-013 stale physical-pixel DPI compensation remains: ${forbidden}`);
if (main.includes("WindowEvent::Resized") || main.includes("window.maximize()") || main.includes("window.unmaximize()"))
  throw new Error("LB-013 fixed window still carries resize/maximize runtime behavior");
if (/notification|toast|banner/i.test(`${main}\n${tray}\n${background}`)) throw new Error("LB-013 added pre-exhaustion notification surface");
for (const required of ["RecoveryOutcome::Exhausted", "user_attention_required", "ShowFinalErrorWindow", "ProductionRuntimeOwner", "runtime: Arc<Mutex<ProductionRuntimeOwner>>", "ProductionRuntimeOwner::default()", "start_production_runtime", "ProductionRuntimeDriver::new_owned", "WindowsCredentialStore::default", "with_privileged_execution", "self.privilege.gateway()", "owner.activate_boxed(runtime)?", "shutdown_in_security_order(Some(&mut *runtime), privilege)"])
  if (!normalized(background).includes(normalized(required))) throw new Error(`LB-013 recovery attention gate missing: ${required}`);
if (background.includes("Mutex<Option<Box<dyn ExitRuntime")) throw new Error("LB-013 still permits the production lifecycle owner itself to be absent");
if (!main.includes("DesktopLifecycle::new(PrivilegeController::new())")) throw new Error("LB-013 production app setup does not construct the runtime owner");

for (const required of ["TrayIconBuilder", "FROZEN_TRAY_ICON_ICO", 'localbridge-tray.ico', "include_bytes!", "select_frozen_ico_frame", "TRAY_LOGICAL_ICON_SIZE", "primary_monitor", "monitor.scale_factor()", "Image::from_bytes(frame.bytes)", '"打开 LocalBridge"', '"退出"'])
  if (!tray.includes(required)) throw new Error(`LB-013 tray contract missing: ${required}`);
for (const forbidden of ["TRAY_ICON_CROP_PERCENT", "tray_icon_from_frozen(&icon)", "default_window_icon()"])
  if (tray.includes(forbidden)) throw new Error(`LB-013 stale tray resampling path remains: ${forbidden}`);
if (/emoji|placeholder|lucide|heroicon|react-icons/i.test(tray)) throw new Error("LB-013 tray uses placeholder/third-party icon surface");
if (!tray.includes("spawn_shutdown_then(move |_| exit_app.exit(0))") || tray.includes("let _ = lifecycle.shutdown();"))
  throw new Error("LB-013 Tray Exit still performs blocking lifecycle shutdown inside the tray callback");
if (!config.bundle?.icon?.includes("../assets/icons/localbridge.ico")) throw new Error("LB-013 bundle icon is not frozen localbridge.ico");
const ico = readFileSync("assets/icons/localbridge.ico");
const hash = createHash("sha256").update(ico).digest("hex");
if (hash !== "c995d6af01ebc5031950eb9ea6415b58671b31f84ed6b55baabe80ea51e33f78") throw new Error("LB-013 frozen taskbar/left-bottom icon hash drift");
const masterPng = readFileSync("assets/icons/localbridge.png");
if (createHash("sha256").update(masterPng).digest("hex") !== "710690f2d70e3c69f13db9d4eaebc0bef5c80561c74acc7bc5a401c15c16e55a") throw new Error("LB-013 frozen master PNG hash drift");
const trayIco = readFileSync("assets/icons/localbridge-tray.ico");
const trayHash = createHash("sha256").update(trayIco).digest("hex");
if (trayHash !== "5a9fce6e80050c9ce1620b8e28767052ece2536213ee04fe532596d3f4ec811d") throw new Error("LB-013 PNG-derived tray icon hash drift");
for (const marker of ["assets/icons/localbridge.png", "$CropX = 90", "$CropY = 400", "$CropWidth = 390", "$CropHeight = 390", "$FrameSizes = @(16, 20, 24, 32, 48)", ".DrawImage($sourceBitmap", "HighQualityBicubic"]) if (!trayDeriver.includes(marker)) throw new Error(`LB-013 tray PNG derivation script missing: ${marker}`);
if (/FillRectangle|FillEllipse|DrawString|DrawIcon|DrawLine|DrawPolygon|GraphicsPath/i.test(trayDeriver)) throw new Error("LB-013 tray derivation introduces newly authored graphics instead of source-PNG-only processing");
const trayFrameSizes = [];
const trayFrameCount = trayIco.readUInt16LE(4);
for (let index = 0; index < trayFrameCount; index += 1) {
  const raw = trayIco[6 + index * 16];
  trayFrameSizes.push(raw === 0 ? 256 : raw);
}
if (JSON.stringify(trayFrameSizes) !== JSON.stringify([16,20,24,32,48])) throw new Error(`LB-013 dedicated tray frame inventory drift: ${trayFrameSizes.join(",")}`);
if (!/select_frozen_ico_frame\(FROZEN_TRAY_ICON_ICO, 1\.25\)[\s\S]*?\.size,[\s\r\n]*20/.test(tray)) throw new Error("LB-013 125% DPI does not select exact 20px tray frame");

const disableStart = privilege.indexOf("pub fn disable");
const disableEnd = privilege.indexOf("pub fn refresh_broker_state", disableStart);
const disable = privilege.slice(disableStart, disableEnd);
if (!(disable.indexOf("gate_open.store(false") >= 0 && disable.indexOf("session.shutdown") > disable.indexOf("gate_open.store(false")))
  throw new Error("LB-013 privilege disable does not close gate before Broker shutdown");
if (!orchestrator.includes("pub fn stop_tunnel_for_exit") || !orchestrator.includes("pub fn finish_exit_after_tunnel"))
  throw new Error("LB-013 production runtime has no staged security-order shutdown");
const backgroundTest = readFileSync("tests/integration/background/background.rs", "utf8");
if (!backgroundTest.includes("production_tray_exit_owns_actual_adapter_and_stops_tunnel_gate_pep_mcp"))
  throw new Error("LB-013 has no actual production-adapter Tray Exit regression");
for (const required of [
  "close_window_policy_defaults_to_continue_running_and_is_memory_cached",
  "backend_shutdown_dispatch_returns_before_deliberately_slow_cleanup_finishes",
]) if (!backgroundTest.includes(required)) throw new Error(`LB-013 blocking-boundary regression missing: ${required}`);
for (const required of ["CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 4", "close_window_continue_running"])
  if (!settingsModel.includes(required)) throw new Error(`LB-013 persisted close policy missing: ${required}`);
for (const required of ["3 => migrate_v3_to_v4(value)?", "close_window_continue_running: true"])
  if (!migration.includes(required)) throw new Error(`LB-013 close policy migration missing: ${required}`);
if (!migrationTest.includes("v3_migrates_close_window_policy_to_safe_continue_running_default"))
  throw new Error("LB-013 has no v3->v4 close-policy migration regression");
if (!startup.includes("set_close_window_continue_running(data.settings.close_window_continue_running)"))
  throw new Error("LB-013 startup does not load persisted close policy into backend memory");
for (const required of ["DesktopBackendHandle", "spawn_shutdown_then", 'name("localbridge-desktop-shutdown"', "backend.shutdown()"])
  if (!background.includes(required)) throw new Error(`LB-013 nonblocking lifecycle boundary missing: ${required}`);
for (const id of ["EXEC-PREAUTH-LB013-001", "EXEC-PREAUTH-LB013-002", "EXEC-PREAUTH-LB013-003", "EXEC-PREAUTH-LB013-004", "EXEC-PREAUTH-LB013-005"]) {
  const record = auth.records.find((candidate) => candidate.authorization_id === id);
  if (!record || record.user_audit_status !== "PENDING" || record.does_not_expand_future_pr_writable_paths !== true)
    throw new Error(`LB-013 preauthorization record invalid: ${id}`);
}
console.log("LB013_CONTRACT=PASS background_no_window=true logical_fixed_window=780x620 native_dpi_scaling=true inverse_webview_zoom=false physical_pixel_lock=false resizable=false maximizable=false decorations=false webview_edge_bound_at_creation=true dpi_change_webview_sync=true close_policy=persisted_v4 hide_or_async_exit=true lifecycle_ui_thread_blocking=false tray_exit_async=true tray_frozen_ico=true tray_native_dpi_frame=true tray_resample_hack=false exit_order=true production_owner_at_app_setup=true runtime_owner_nonoptional=true actual_adapter_shutdown_test=true recovery_silent_until_exhaustion=true preauth_pending=5");
