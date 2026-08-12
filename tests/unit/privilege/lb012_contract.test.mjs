import { readFileSync } from "node:fs";

const read = (path) => readFileSync(path, "utf8");
const control = read("src-tauri/src/privilege/control.rs");
const state = read("src-tauri/src/state/privilege.rs");
const policyToml = read("runtime-policy.toml");
const policy = read("src-tauri/src/mcp/policy.rs");
const guard = read("src-tauri/src/mcp/guard.rs");
const server = read("src-tauri/src/mcp/server.rs");
const execution = read("src-tauri/src/privilege/execution.rs");
const orchestrator = read("src-tauri/src/runtime/orchestrator.rs");
const main = read("src-tauri/src/main.rs");
const lib = read("src-tauri/src/lib.rs");
const tauri = read("src-tauri/tauri.conf.json");

for (const forbidden of ["expires_at", "expiresAt", "expiry", "admin_ttl", "privilege_ttl", "lease_deadline", "lease_expires"]) {
  if (`${state}\n${control}`.toLowerCase().includes(forbidden.toLowerCase())) {
    throw new Error(`LB-012 hidden elevation lease detected: ${forbidden}`);
  }
}
if (!/\[elevation\][\s\S]*?ttl\s*=\s*"none"/.test(policyToml)) throw new Error("LB-012 elevation TTL is not explicitly none");
if (!policyToml.includes('automatic_background_uac = "forbidden"')) throw new Error("LB-012 background UAC policy missing");

const enableStart = control.indexOf("pub fn enable_from_explicit_user_action");
const enableEnd = control.indexOf("pub fn disable", enableStart);
const enable = control.slice(enableStart, enableEnd);
if (!enable.includes("launch_broker_with_explicit_uac")) throw new Error("LB-012 explicit enable does not own UAC launch");
for (const source of [main, lib, orchestrator]) {
  if (source.includes("launch_broker_with_explicit_uac") || source.includes("enable_from_explicit_user_action")) {
    throw new Error("LB-012 normal/background runtime can directly trigger UAC");
  }
}
if (!orchestrator.includes("privileged_execution: None")) throw new Error("LB-012 production runtime does not default privileged route to None");
if (!orchestrator.includes("with_privileged_execution") || !orchestrator.includes("start_with_privilege")) throw new Error("LB-012 explicit stable-adapter privileged injection missing");

const disableStart = control.indexOf("pub fn disable");
const disableEnd = control.indexOf("pub fn refresh_broker_state", disableStart);
const disable = control.slice(disableStart, disableEnd);
const closeGate = disable.indexOf("gate_open.store(false");
const shutdown = disable.indexOf("session.shutdown");
if (!(closeGate >= 0 && shutdown > closeGate)) throw new Error("LB-012 disable does not close call gate before Broker shutdown");
for (const method of ["start_execute", "poll_execute", "cancel_execute"]) {
  const start = control.indexOf(`fn ${method}`, control.indexOf("impl PrivilegedExecution for PrivilegedExecutionGateway"));
  if (start < 0 || !control.slice(start, start + 700).includes("require_gate()?")) throw new Error(`LB-012 ${method} does not re-check Active gate`);
}

for (const required of [
  'elevated_exec_in_edit = "deny"',
  'elevated_exec_in_full = "deny"',
  'elevated_exec_in_elevated = "allow_if_reviewed_and_broker_active"',
  'canonical_request = "structured_program_args"',
  'shell_true_default = false',
  'requires_broker = true',
]) if (!policyToml.includes(required)) throw new Error(`LB-012 runtime policy missing: ${required}`);
if (!policy.includes('if name == "elevated_exec"') || !policy.includes("Capability::ElevatedExec")) throw new Error("LB-012 elevated_exec capability classification missing");
if (!guard.includes('name != "elevated_exec"') || !guard.includes("PrivilegedRouteNotAvailable")) throw new Error("LB-012 ordinary upstream route does not reserve elevated_exec");

const handlerStart = server.indexOf("fn handle_elevated_exec");
const handlerEnd = server.indexOf("fn request_id", handlerStart);
if (handlerStart < 0 || handlerEnd <= handlerStart) throw new Error("LB-012 elevated_exec PEP handler missing");
const handler = server.slice(handlerStart, handlerEnd);
for (const required of ["privileged.start_execute", "privileged.poll_execute", "PrivilegeState::Active", "TaskExecutionState::AwaitingAuthorization", "TaskExecutionState::Blocked", "SafeTaskSummary::Omitted"]) {
  const source = required === "SafeTaskSummary::Omitted" ? server : handler;
  if (!source.includes(required)) throw new Error(`LB-012 broker route missing: ${required}`);
}
if (handler.includes("guard.call_tool") || handler.includes("raw_call_tool") || handler.includes("call_tool_with_request_id")) throw new Error("LB-012 elevated_exec can reach ordinary upstream MCP route");

for (const required of ["CreateProcessW", "CREATE_SUSPENDED", "AssignProcessToJobObject", "ResumeThread", "CREATE_NO_WINDOW", "JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE"]) {
  if (!execution.includes(required)) throw new Error(`LB-012 Job-owned structured execution missing: ${required}`);
}
const assign = execution.indexOf("AssignProcessToJobObject(job, process_info.hProcess)");
const resume = execution.indexOf("ResumeThread(process_info.hThread)");
if (!(assign >= 0 && resume > assign)) throw new Error("LB-012 elevated process resumes before Job assignment");
if (execution.includes("std::process::Command") || execution.includes("Command::new(")) throw new Error("LB-012 elevated execution has process/shell fallback");
if (/requireAdministrator|highestAvailable/i.test(tauri)) throw new Error("LB-012 whole LocalBridge app requests elevation");

console.log("LB012_CONTRACT=PASS no_ttl=true explicit_uac=true active_gate=true broker_only=true structured_exec=true no_auto_uac=true");
