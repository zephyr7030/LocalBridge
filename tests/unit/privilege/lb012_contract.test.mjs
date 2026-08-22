import { readFileSync } from "node:fs";

const read = (path) => readFileSync(path, "utf8");
const control = read("src-tauri/src/privilege/control.rs");
const state = read("src-tauri/src/state/privilege.rs");
const policyToml = read("runtime-policy.toml");
const policy = read("src-tauri/src/mcp/policy.rs");
const guard = read("src-tauri/src/mcp/guard.rs");
const server = read("src-tauri/src/mcp/server.rs");
const shell = read("src-tauri/src/mcp/shell.rs");
const execution = read("src-tauri/src/privilege/execution.rs");
const filesystem = read("src-tauri/src/privilege/filesystem.rs");
const orchestrator = read("src-tauri/src/runtime/orchestrator.rs");
const main = read("src-tauri/src/main.rs");
const lib = read("src-tauri/src/lib.rs");
const tauri = read("src-tauri/tauri.conf.json");
const facade = read("src-tauri/src/mcp/facade.rs");
const toolbox = read("src-tauri/src/mcp/toolbox.rs");
const runtimeManifest = read("runtime-manifest.toml");
const toolboxPrepare = read("scripts/prepare-toolbox.mjs");
const thirdPartyNotices = read("THIRD_PARTY_NOTICES.md");
const packageJson = JSON.parse(read("package.json"));

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
for (const method of ["start_execute", "poll_execute", "cancel_execute", "filesystem"]) {
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
  'review_model = "exact_trusted_program_and_args"',
  'arbitrary_programs = "deny"',
  'shells_and_interpreters = "deny"',
  'control_plane_mutation = "deny_always"',
]) if (!policyToml.includes(required)) throw new Error(`LB-012 runtime policy missing: ${required}`);
for (const required of [
  '[administrator_gateway]',
  'route = "broker_only"',
  'token_scope = "administrator_token"',
  'direct_process = "structured_absolute_program_argv"',
  'shell = "trusted_logical_selector_only"',
  'filesystem = "structured_absolute_path_broker"',
  'system_management_identity = "exact_system32"',
  'arbitrary_shell_executable_path = "deny"',
]) if (!policyToml.includes(required)) throw new Error(`LB-012 administrator gateway policy missing: ${required}`);
if (!policy.includes('if name == "elevated_exec"') || !policy.includes("Capability::ElevatedExec")) throw new Error("LB-012 elevated_exec capability classification missing");
for (const required of ["decide_request", "reviewed_elevated_exec", "reviewed_elevated_program", "GetSystemDirectoryW", "whoami.exe", "ElevatedExecNotReviewed"]) {
  if (!policy.includes(required)) throw new Error(`LB-012 reviewed elevated_exec enforcement missing: ${required}`);
}
for (const required of ["reviewed_administrator_process", "reviewed_administrator_shell", "reviewed_administrator_filesystem", "trusted_system_program", "administrator_shell_executable", "explicit_control_plane_reference"]) {
  if (!policy.includes(required)) throw new Error(`LB-012 schema33 administrator review missing: ${required}`);
}
if (!guard.includes('name != "elevated_exec"') || !guard.includes("PrivilegedRouteNotAvailable")) throw new Error("LB-012 ordinary upstream route does not reserve elevated_exec");

const handlerStart = server.indexOf("fn handle_elevated_exec");
const handlerEnd = server.indexOf("fn request_id", handlerStart);
if (handlerStart < 0 || handlerEnd <= handlerStart) throw new Error("LB-012 elevated_exec PEP handler missing");
const handler = server.slice(handlerStart, handlerEnd);
const reviewSnapshot = handler.indexOf("let reviewed_arguments = arguments.clone()");
const realArgumentDecision = handler.indexOf("execution_guard.elevated_decision(mode, &reviewed_arguments)");
const structuredParse = handler.indexOf("elevated_exec_spec(arguments)");
if (!(reviewSnapshot >= 0 && realArgumentDecision > reviewSnapshot && structuredParse > realArgumentDecision) || handler.includes('ToolCallRequest::new("elevated_exec", json!({}))')) {
  throw new Error("LB-012 policy decision does not consume real elevated_exec arguments before structured dispatch");
}
if (!/let\s+(?:mut\s+)?execution_guard\s*=\s*guard/.test(handler) || !handler.includes("drop(execution_guard)")) throw new Error("LB-012 elevated execution is not serialized by the Guard execution mutex");
for (const required of ['"enum": ["process", "shell", "filesystem"]', 'Some("process") =>', 'Some("shell") =>', 'Some("filesystem") =>']) {
  if (!server.includes(required)) throw new Error(`LB-012 typed elevated_exec schema missing: ${required}`);
}
const elevatedToolStart = server.indexOf('"name": "elevated_exec"');
const elevatedToolEnd = server.indexOf("fn elevated_exec_output_schema", elevatedToolStart);
if (elevatedToolStart < 0 || elevatedToolEnd <= elevatedToolStart || server.slice(elevatedToolStart, elevatedToolEnd).includes('"oneOf"')) {
  throw new Error("LB-012 elevated_exec public input schema regressed to a client-hostile top-level combinator");
}
if (!handler.includes("privileged.filesystem(spec)")) throw new Error("LB-012 privileged filesystem does not dispatch directly to Broker gateway");
for (const required of ["execution.stdout", "execution.stderr", "retain_local_output", '"output_refs": output_refs']) {
  if (!handler.includes(required)) throw new Error(`LB-012 structured elevated output/continuation missing: ${required}`);
}
const elevatedOutputSchemaStart = server.indexOf("fn elevated_exec_output_schema");
const elevatedOutputSchemaEnd = server.indexOf("fn privileged_request_id", elevatedOutputSchemaStart);
const elevatedOutputSchema = server.slice(elevatedOutputSchemaStart, elevatedOutputSchemaEnd);
for (const required of ['"stdout"', '"stderr"', '"stdout_truncated"', '"stderr_truncated"', '"output_refs"']) {
  if (!elevatedOutputSchema.includes(required)) throw new Error(`LB-012 elevated output schema missing ${required}`);
}
if (!server.includes("broker_direct_spec(&shell_spec)")) throw new Error("LB-012 shell route does not use Broker-only trusted shell preparation");
if (!shell.includes("resolve_for_broker") || !shell.includes("highest_core_for_broker")) throw new Error("LB-012 Broker shell resolver missing");
const brokerResolverStart = shell.indexOf("pub fn resolve_for_broker");
const ordinaryHighestCoreStart = shell.indexOf("fn highest_core(", brokerResolverStart);
if (brokerResolverStart < 0 || ordinaryHighestCoreStart <= brokerResolverStart || shell.slice(brokerResolverStart, ordinaryHighestCoreStart).includes("probe_powershell_core")) {
  throw new Error("LB-012 Broker shell resolver executes a version probe under the ordinary token");
}
if (/Command::new|powershell\.exe|cmd\.exe/i.test(filesystem)) throw new Error("LB-012 privileged filesystem is shell/process backed");
const toolsListStart = server.indexOf('"tools/list" =>');
const toolsCallStart = server.indexOf('"tools/call" =>', toolsListStart);
if (toolsListStart < 0 || toolsCallStart <= toolsListStart) throw new Error("LB-012 tools/list branch missing");
const toolsList = server.slice(toolsListStart, toolsCallStart);
const catalogStart = server.indexOf("fn effective_tool_catalog(");
const catalogEnd = server.indexOf("fn effective_tool_catalog_signature(", catalogStart);
const effectiveCatalog = server.slice(catalogStart, catalogEnd);
if (
  !toolsList.includes("effective_tool_catalog(&policy, mode)") ||
  catalogStart < 0 ||
  catalogEnd <= catalogStart ||
  effectiveCatalog.includes("accepts_privileged_calls()") ||
  !effectiveCatalog.includes('privileged_tool_visible(mode, "elevated_exec")') ||
  !effectiveCatalog.includes("append_elevated_exec_tool(&mut result)")
) {
  throw new Error("LB-012 tools/list does not stably advertise elevated_exec independently of Broker state");
}
const gatewayImplStart = control.indexOf("impl PrivilegedExecutionGateway");
const gatewayStateStart = control.indexOf("pub fn state(&self) -> PrivilegeState", gatewayImplStart);
const gatewayExecuteStart = control.indexOf("pub fn execute", gatewayStateStart);
if (gatewayImplStart < 0 || gatewayStateStart < 0 || gatewayExecuteStart <= gatewayStateStart) {
  throw new Error("LB-012 privileged gateway state method missing");
}
const gatewayState = control.slice(gatewayStateStart, gatewayExecuteStart);
if (!gatewayState.includes("refresh_broker_liveness")) {
  throw new Error("LB-012 tools/list can observe cached Active without refreshing Broker process liveness");
}
for (const required of ["if !matches!(privileged.state(), PrivilegeState::Active { .. })", "privileged.start_execute", "privileged.poll_execute", "PrivilegeState::Active", "TaskExecutionState::AwaitingAuthorization", "TaskExecutionState::Blocked", "SafeTaskSummary::Omitted"]) {
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

for (const required of [
  "pub const V1_CORE_TOOL_NAMES: [&str; 8]",
  "CapabilityUnavailable",
  "ToolboxResolver::probe(runtime.install_root())",
  '"PATH":self.toolbox.child_path()',
  '"NoDefaultCurrentDirectoryInExePath":"1"',
]) if (!facade.includes(required)) throw new Error(`LB-012 schema46 Toolbox environment integration missing: ${required}`);
for (const required of [
  "struct ToolboxResolver",
  "run_bounded_command",
  'join("System32")',
  'join("curl.exe")',
  'output.contains("protocols:")',
  'output.contains("http")',
  'output.contains("https")',
  "ARIA2C_SHA256",
  "SEVEN_ZIP_SHA256",
  "JQ_SHA256",
]) if (!toolbox.includes(required)) throw new Error(`LB-012 schema42 Toolbox resolver missing: ${required}`);
if (/https?:\/\//i.test(toolbox) || toolbox.includes("reqwest") || toolbox.includes("set_var(\"PATH\"") || toolbox.includes("setx")) throw new Error("LB-012 Toolbox runtime contains downloader or persistent PATH mutation");
for (const required of [
  'runtime_download = false',
  'runtime_update = false',
  'persistent_path_mutation = false',
  'public_tools = false',
  'version = "1.37.0"',
  'version = "26.02"',
  'version = "1.8.2"',
  'executable = "%SystemRoot%/System32/curl.exe"',
  'capability_missing_error = "CapabilityUnavailable"',
]) if (!runtimeManifest.includes(required)) throw new Error(`LB-012 schema42 Toolbox manifest missing: ${required}`);
if (packageJson.scripts?.["toolbox:prepare"] !== "node scripts/prepare-toolbox.mjs") throw new Error("LB-012 Toolbox build preparation script missing");
if (!tauri.includes('"beforeDevCommand": "npm run dev"') || !tauri.includes('"beforeBuildCommand": "npm run toolbox:prepare && npm run build"') || !tauri.includes('"target/toolbox-stage/": "runtime/toolbox/"')) throw new Error("LB-012 Toolbox is not build-only packaged resource");
for (const pin of [
  "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288",
  "081df9e9311dfd9c9e0e98c1c80180b99bb51e4cb24156b5f3057fe3c259d70a",
  "a6fc67fedaf9128a3309a1e2ebb8b986aeccf70122ee46d2cb4849e423f0c627",
]) if (!toolboxPrepare.includes(pin) || !thirdPartyNotices.includes(pin)) throw new Error(`LB-012 Toolbox provenance pin missing: ${pin}`);
if (!shell.includes("Remove-Item Alias:curl -Force -ErrorAction SilentlyContinue")) throw new Error("LB-012 PowerShell curl alias can bypass exact System32 resolver");

console.log("LB012_CONTRACT=PASS no_ttl=true explicit_uac=true active_gate=true broker_only=true administrator_gateway=true privileged_filesystem=true trusted_shell=true structured_exec=true toolbox=true no_auto_uac=true");
