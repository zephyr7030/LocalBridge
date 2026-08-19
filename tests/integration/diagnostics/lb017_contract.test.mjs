import { readFileSync } from "node:fs";
const ui=readFileSync("src/features/diagnostics/Diagnostics.tsx","utf8");
const api=readFileSync("src/features/diagnostics/api.ts","utf8");
const commands=readFileSync("src-tauri/src/commands/diagnostics.rs","utf8");
const model=readFileSync("src-tauri/src/diagnostics/mod.rs","utf8");
const lib=readFileSync("src-tauri/src/lib.rs","utf8");
const auth=JSON.parse(readFileSync("scripts/authorization-records/LB-017.json","utf8"));
for(const title of [">运行状态<",">项目<",">日志<"]) if(!ui.includes(title)) throw new Error(`LB-017 section missing: ${title}`);
for(const action of [">打开日志<",">导出诊断<",">完成<"]) if(!ui.includes(action)) throw new Error(`LB-017 action missing: ${action}`);
for(const forbidden of [">刷新<",">重试连接<",">打开欢迎页<","实例代次","恢复代次","第 {attempt.attempt} 次","Broker PID","nonce","SID","IPC"]) if(ui.includes(forbidden)) throw new Error(`LB-017 engineering/duplicate UI remains: ${forbidden}`);
if(!ui.includes("snapshot.activeWorkspacePath") || !api.includes("activeWorkspacePath: string | null")) throw new Error("LB-017 does not show actual current project path");
if(!ui.includes("snapshot.recentEvents") || !api.includes("recentEvents:")) throw new Error("LB-017 bounded recent user log projection missing");
for(const required of ["RECENT_EVENT_LIMIT: usize = 8","VecDeque","DiagnosticEvent","timestamp_ms","record_runtime_user_events","events.truncate(RECENT_EVENT_LIMIT)","runtime_fault_label","active_workspace_path"]) if(!model.includes(required)) throw new Error(`LB-017 typed/redacted event projection missing: ${required}`);
for(const required of ["pub async fn open_logs","tauri::async_runtime::spawn_blocking","app_data_dir()",'.join("logs")','Command::new("explorer.exe")']) if(!commands.includes(required)) throw new Error(`LB-017 fixed log directory action missing: ${required}`);
for(const command of ["get_diagnostics","open_logs","export_diagnostics"]) { const start=commands.indexOf("pub async fn "+command); if(start<0) throw new Error(`LB-017 blocking command remains: ${command}`); const next=commands.indexOf("pub async fn ",start+12); const body=commands.slice(start,next<0?commands.length:next); if(!body.includes("spawn_blocking")) throw new Error(`LB-017 command lacks worker boundary: ${command}`); }
for(const required of ['object.remove("activeWorkspacePath")','object.remove("runtimeKeyPresent")','Some("runtime_key")']) if(!model.includes(required)) throw new Error(`LB-017 export redaction missing: ${required}`);
for(const required of ["REQUEST_DIAGNOSTIC_LIMIT", "RequestDiagnosticEvent", "request_id", "connection_id", "duration_ms", "record_runtime_request_diagnostics", 'format!("req-recovery-{}", outage.generation)', 'format!("conn-recovery-{}-{attempt}", outage.generation)']) if(!model.includes(required)) throw new Error(`LB-017 schema42 request diagnostics missing: ${required}`);
for(const forbidden of ["requestDiagnostics", "requestId", "connectionId", "errorCode", "httpStatus", "durationMs"]) if(api.includes(forbidden) || ui.includes(forbidden)) throw new Error(`LB-017 normal WebView API leaked request engineering field: ${forbidden}`);
for(const forbidden of ["runtimeKeyPresent", "ReconnectDiagnostics", "ReconnectAttempt", "generation: number | null", "schemaVersion: number"]) if(api.includes(forbidden)) throw new Error(`LB-017 normal WebView API still exposes engineering/internal field: ${forbidden}`);
for(const required of ["pub struct DiagnosticsViewProjection", "privilege: BrokerDiagnosticState", "project_diagnostics_view", '.filter(|check| check.code != "runtime_key")', "SettingsStore::new", "settings.workspace.active_entry()", "WorkspaceValidator", "entry.validated_identity.as_str() != validated.identity().as_str()", "validated.execution_path().to_string_lossy().into_owned()"])
  if(!commands.includes(required)) throw new Error(`LB-017 safe minimal WebView projection/current-project validation missing: ${required}`);
const getStart=commands.indexOf("pub async fn get_diagnostics"); const getEnd=commands.indexOf("fn get_diagnostics_snapshot_blocking",getStart); const getBody=commands.slice(getStart,getEnd); if(getBody.includes("Result<DiagnosticsSnapshot") || getBody.includes("runtime_key_present") || getBody.includes("reconnect_diagnostics")) throw new Error("LB-017 get_diagnostics still serializes full internal diagnostics snapshot");
if(!api.includes("timestampMs: number") || !ui.includes("event.timestampMs")) throw new Error("LB-017 recent events are not rendered from backend timestamps");
if(commands.includes("diagnostics_retry_connection") || lib.includes("diagnostics_retry_connection") || api.includes("retry:")) throw new Error("LB-017 diagnostics retry action remains");
if(!commands.includes("export_snapshot") || !model.includes("export_snapshot")) throw new Error("LB-017 redacted export path missing");
const latest=auth.records.find(r=>r.authorization_id==="EXEC-PREAUTH-LB017-002"); if(!latest || latest.user_audit_status!=="PENDING") throw new Error("LB-017 historical audit record changed");
console.log("LB017_CONTRACT=PASS sections=3 actions=3 engineering_details_hidden=true project_path=true bounded_redacted_events=true fixed_log_directory=true export_redacted=true");
