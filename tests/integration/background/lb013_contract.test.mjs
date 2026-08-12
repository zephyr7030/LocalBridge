import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const main = readFileSync("src-tauri/src/main.rs", "utf8");
const tray = readFileSync("src-tauri/src/tray/mod.rs", "utf8");
const background = readFileSync("src-tauri/src/app/background.rs", "utf8");
const privilege = readFileSync("src-tauri/src/privilege/control.rs", "utf8");
const orchestrator = readFileSync("src-tauri/src/runtime/orchestrator.rs", "utf8");
const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const auth = JSON.parse(readFileSync("scripts/authorization-records/LB-013.json", "utf8"));

if ((config.app?.windows ?? []).length !== 0) throw new Error("LB-013 background startup still has a static Tauri main window");
if (!main.includes('windows_subsystem = "windows"')) throw new Error("LB-013 LocalBridge app binary can still expose a console window");
for (const required of ["StartupMode::from_args", "creates_main_window_at_startup", "ensure_main_window", "install_tray", "CloseRequested", "prevent_close", ".hide()"])
  if (!main.includes(required)) throw new Error(`LB-013 entry lifecycle missing: ${required}`);
const modeGuard = main.indexOf("creates_main_window_at_startup");
const createCall = main.indexOf("ensure_main_window", modeGuard);
if (!(modeGuard >= 0 && createCall > modeGuard)) throw new Error("LB-013 foreground window creation is not startup-mode gated");
if (/notification|toast|banner/i.test(`${main}\n${tray}\n${background}`)) throw new Error("LB-013 added pre-exhaustion notification surface");
for (const required of ["RecoveryOutcome::Exhausted", "user_attention_required", "ShowFinalErrorWindow"])
  if (!background.includes(required)) throw new Error(`LB-013 recovery attention gate missing: ${required}`);

for (const required of ["TrayIconBuilder", "default_window_icon", '"打开 LocalBridge"', '"退出"'])
  if (!tray.includes(required)) throw new Error(`LB-013 tray contract missing: ${required}`);
if (/emoji|placeholder|lucide|heroicon|react-icons/i.test(tray)) throw new Error("LB-013 tray uses placeholder/third-party icon surface");
if (!config.bundle?.icon?.includes("../assets/icons/localbridge.ico")) throw new Error("LB-013 bundle icon is not frozen localbridge.ico");
const ico = readFileSync("assets/icons/localbridge.ico");
const hash = createHash("sha256").update(ico).digest("hex");
if (hash !== "c995d6af01ebc5031950eb9ea6415b58671b31f84ed6b55baabe80ea51e33f78") throw new Error("LB-013 frozen tray icon hash drift");

const disableStart = privilege.indexOf("pub fn disable");
const disableEnd = privilege.indexOf("pub fn refresh_broker_state", disableStart);
const disable = privilege.slice(disableStart, disableEnd);
if (!(disable.indexOf("gate_open.store(false") >= 0 && disable.indexOf("session.shutdown") > disable.indexOf("gate_open.store(false")))
  throw new Error("LB-013 privilege disable does not close gate before Broker shutdown");
if (!orchestrator.includes("pub fn stop_tunnel_for_exit") || !orchestrator.includes("pub fn finish_exit_after_tunnel"))
  throw new Error("LB-013 production runtime has no staged security-order shutdown");
for (const id of ["EXEC-PREAUTH-LB013-001", "EXEC-PREAUTH-LB013-002"]) {
  const record = auth.records.find((candidate) => candidate.authorization_id === id);
  if (!record || record.user_audit_status !== "PENDING" || record.does_not_expand_future_pr_writable_paths !== true)
    throw new Error(`LB-013 preauthorization record invalid: ${id}`);
}
console.log("LB013_CONTRACT=PASS background_no_window=true close_to_hide=true tray_frozen_icon=true exit_order=true recovery_silent_until_exhaustion=true preauth_pending=2");
