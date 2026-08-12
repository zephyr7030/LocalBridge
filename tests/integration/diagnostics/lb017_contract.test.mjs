import { readFileSync } from "node:fs";

const model = readFileSync("src-tauri/src/diagnostics/mod.rs", "utf8");
const commands = readFileSync("src-tauri/src/commands/diagnostics.rs", "utf8");
const ui = readFileSync("src/features/diagnostics/Diagnostics.tsx", "utf8");
const api = readFileSync("src/features/diagnostics/api.ts", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
const lib = readFileSync("src-tauri/src/lib.rs", "utf8");
const auth = JSON.parse(readFileSync("scripts/authorization-records/LB-017.json", "utf8"));

const generationLabels = (ui.match(/>\s*Generation\s*</g) ?? []).length;
if (generationLabels !== 0) throw new Error(`LB-017 user-visible Generation labels remain: ${generationLabels}`);
for (const required of [">实例代次<", ">恢复代次<"]) if (!ui.includes(required)) throw new Error(`LB-017 Chinese generation label missing: ${required}`);

for (const required of ["DiagnosticLevel", "DiagnosticCheck", "BrokerDiagnostics", "ReconnectDiagnostics", "DIAGNOSTICS_SCHEMA_VERSION", "export_snapshot"]) if (!model.includes(required)) throw new Error(`LB-017 typed diagnostics missing: ${required}`);
for (const forbidden of ["SecretString", "expose_secret", "credential_id", "pipe_name", "process_snapshot", "expected_pid", "SessionNonce"]) if (model.includes(forbidden)) throw new Error(`LB-017 diagnostics model contains forbidden secret/internal surface: ${forbidden}`);
if (model.includes("process::id") || commands.includes("process::id")) throw new Error("LB-017 diagnostics exposes process ID in export/artifact naming");
if (!commands.includes("runtime_api_key_metadata") || !commands.includes("metadata.has_runtime_key")) throw new Error("LB-017 credential check is not metadata-only");
for (const forbidden of ["save_runtime_api_key", "remove_runtime_api_key", "set_runtime_permission_mode", "switch_runtime_workspace", "add_project", "select_project", "remove_project", "enable_from_explicit_user_action", "ShellExecuteW", "Command::new"])
  if (`${commands}\n${model}`.includes(forbidden)) throw new Error(`LB-017 unsafe repair/mutation boundary: ${forbidden}`);
if (!commands.includes("manual_retry_after_attention")) throw new Error("LB-017 safe retry does not use accepted bounded recovery control");

if (!model.includes("broker_generation.get()") || !ui.includes("snapshot.broker.generation")) throw new Error("LB-017 broker generation diagnostics missing");
for (const forbidden of ["nonce", "SID", "pipe", "PID", "secret"]) if (ui.includes(forbidden)) throw new Error(`LB-017 UI exposes broker internal ${forbidden}`);
if (!model.includes("RecoveryDisposition::Recoverable") || !model.includes("(1..=5)")) throw new Error("LB-017 exhausted reconnect history is not grounded in frozen recovery semantics");
if (!ui.includes("snapshot.reconnect.attempts") || !ui.includes("第 {attempt.attempt} 次")) throw new Error("LB-017 diagnostics cannot display reconnect attempt history");
if (app.includes("snapshot.reconnect.attempts") || app.includes("ReconnectAttempt")) throw new Error("LB-017 reconnect attempt history leaked into dashboard");

if (!ui.includes("导出诊断") || !api.includes('invoke<string>("export_diagnostics")')) throw new Error("LB-017 user-triggered redacted export missing");
const initialEffect = ui.slice(ui.indexOf("useEffect("), ui.indexOf("const retry"));
if (initialEffect.includes("exportReport") || initialEffect.includes("diagnosticsApi.exportReport")) throw new Error("LB-017 diagnostics export is automatic rather than user-triggered");
for (const command of ["get_diagnostics", "diagnostics_retry_connection", "export_diagnostics"]) if (!lib.includes(`commands::diagnostics::${command}`)) throw new Error(`LB-017 command not registered: ${command}`);
if (!app.includes('<Diagnostics onClose={() => setView("main")} />')) throw new Error("LB-017 feature is not composed into existing diagnostics entry");

const record = auth.records.find((candidate) => candidate.authorization_id === "EXEC-PREAUTH-LB017-001");
const expectedScope = ["src/App.tsx", "src-tauri/src/lib.rs", "scripts/authorization-records/LB-017.json"];
if (!record || record.user_audit_status !== "PENDING" || record.does_not_expand_future_pr_writable_paths !== true || JSON.stringify(record.scope) !== JSON.stringify(expectedScope)) throw new Error("LB-017 preauthorization invalid");

const languageRecord = auth.records.find((candidate) => candidate.authorization_id === "EXEC-PREAUTH-LB017-002");
if (!languageRecord || languageRecord.user_audit_status !== "PENDING" || languageRecord.does_not_expand_future_pr_writable_paths !== true || !languageRecord.scope.includes("scripts/verify-ui-language/index.mjs")) throw new Error("LB-017 UI-language preauthorization invalid");
console.log("LB017_CONTRACT=PASS typed_checks=true redacted_export=true broker_generation_safe=true reconnect_history_diagnostics_only=true safe_repair_retry_only=true user_visible_generation=0 preauth_pending=2");
