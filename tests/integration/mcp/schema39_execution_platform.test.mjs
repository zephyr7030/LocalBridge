import { readFileSync } from "node:fs";

const read = (path) => readFileSync(path, "utf8");
const contracts = JSON.parse(read("PR_CONTRACTS.json"));
const facade = read("src-tauri/src/mcp/facade.rs");
const server = read("src-tauri/src/mcp/server.rs");
const taskState = read("src-tauri/src/mcp/task_state.rs");
const checkpoint = read("src-tauri/src/mcp/workflow_checkpoint.rs");

const shell = read("src-tauri/src/mcp/shell.rs");
const runtimeProcesses = read("runtime/coding-tools-mcp/coding_tools_mcp/processes.py");
const runtimeServer = read("runtime/coding-tools-mcp/coding_tools_mcp/server.py");

const fail = (message) => { throw new Error(`SCHEMA42_EXECUTION_PLATFORM: ${message}`); };
if (contracts.schema_version !== 42) fail("contract is not schema42");
if (!facade.includes("pub const AGENT_API_REVISION: u32 = 42")) fail("public facade revision42 missing");

for (const marker of [
  '"workspace_context"', '"agent_workflow"', '"exec_command"', '"command_control"',
  '"task_control"', '"git_workflow"', '"document_workflow"', '"view_image"',
]) if (!facade.includes(marker)) fail(`core tool missing: ${marker}`);
if (!facade.includes("pub const V1_CORE_TOOL_NAMES: [&str; 8]")) fail("non-privileged public core surface is not fixed at eight tools");

for (const marker of [
  '"listChanged": true',
  'format!("{}+api{}", env!("CARGO_PKG_VERSION"), AGENT_API_REVISION)',
  '"api_revision": AGENT_API_REVISION',
  '"catalog": effective_tool_catalog(policy, mode)',
  "tools_list_changed_pending",
  "notifications/tools/list_changed",
  "write_sse_notification",
]) if (!server.includes(marker)) fail(`schema refresh marker missing: ${marker}`);
if (server.includes("effective_tool_catalog_signature(&guard, mode)")) fail("schema signature still depends on the facade execution mutex");

const commandSchemaStart = facade.indexOf('"command_control" => (');
const commandSchemaEnd = facade.indexOf('"task_control" => (', commandSchemaStart);
if (commandSchemaStart < 0 || commandSchemaEnd <= commandSchemaStart) fail("command_control schema block missing");
const commandSchema = facade.slice(commandSchemaStart, commandSchemaEnd);
if (commandSchema.includes('"oneOf"')) fail("command_control regressed to client-hostile top-level oneOf");
for (const field of ["session_id","output_ref","chars","signal","wait_ms","stream","offset","limit"]) {
  if (!commandSchema.includes(`"${field}"`)) fail(`command_control discoverable field missing: ${field}`);
}
const elevatedSchemaStart = server.indexOf('"name": "elevated_exec"');
const elevatedSchemaEnd = server.indexOf("fn elevated_exec_output_schema", elevatedSchemaStart);
if (elevatedSchemaStart < 0 || elevatedSchemaEnd <= elevatedSchemaStart) fail("elevated_exec schema block missing");
const elevatedSchema = server.slice(elevatedSchemaStart, elevatedSchemaEnd);
if (elevatedSchema.includes('"oneOf"')) fail("elevated_exec regressed to client-hostile top-level oneOf");
for (const field of ["operation","program","args","shell","command","workdir","action","path","timeout_ms","max_output_bytes"]) {
  if (!elevatedSchema.includes(`"${field}"`)) fail(`elevated_exec discoverable field missing: ${field}`);
}

const adapterImplStart = facade.indexOf("impl WorkspaceRuntimeAdapter for CodingToolsRuntimeAdapter");
const adapterImplEnd = facade.indexOf("impl CodingToolsRuntimeAdapter {", adapterImplStart);
if (adapterImplStart < 0 || adapterImplEnd <= adapterImplStart) fail("runtime adapter implementation missing");
const adapterImpl = facade.slice(adapterImplStart, adapterImplEnd);
const workspaceMethodStart = adapterImpl.indexOf("fn workspace_context(");
const normalizePathStart = adapterImpl.indexOf("fn normalize_workspace_path(", workspaceMethodStart);
const workspaceMethod = adapterImpl.slice(workspaceMethodStart, normalizePathStart);
if (!workspaceMethod.includes("cached_default_cwd") || !workspaceMethod.includes("cached_project_discovery")) fail("workspace_context does not consume cached compact discovery");
if (workspaceMethod.includes('private_call("get_default_cwd"')) fail("workspace_context still performs a per-call private cwd probe");
const negotiateStart = adapterImpl.indexOf("fn negotiate(");
const workspaceStart = adapterImpl.indexOf("fn workspace_context(", negotiateStart);
const negotiate = adapterImpl.slice(negotiateStart, workspaceStart);
if ((negotiate.match(/private_call\("get_default_cwd"/g) ?? []).length !== 1) fail("default cwd must be probed exactly once during adapter negotiation");
for (const field of [
  "project_name","project_type","project_version","git_branch","git_dirty","git_changed_count",
  "package_manager","build_system","test_system","runtime_availability","trusted_shells","current_task",
]) if (!facade.includes(`"${field}"`)) fail(`compact first-turn context field missing: ${field}`);
if (!server.includes("merge_task_aggregate_activity(aggregate, current_task)")) fail("workspace_context TaskAggregate activity merge missing");

for (const marker of [
  "WorkflowCheckpointStore", "resume_agent_workflow", "load_workflow_checkpoint",
  "save_workflow_checkpoint", "durable_command_terminal", "directory_inflight", "patch_inflight",
  "command_inflight", "agent_workflow_resume_from_fresh_facade_continues_only_missing_steps",
  "agent_workflow_resume_fails_closed_for_uncertain_inflight_file_step",
]) if (!(facade + checkpoint).includes(marker)) fail(`durable resume marker missing: ${marker}`);
for (const marker of ["CryptProtectData", "CryptUnprotectData", 'join("LocalBridge")', 'join("task-state")']) {
  if (!checkpoint.includes(marker)) fail(`secure checkpoint persistence marker missing: ${marker}`);
}
if (!checkpoint.includes("MAX_CHECKPOINT_PLAINTEXT_BYTES") || !checkpoint.includes("MAX_CHECKPOINT_CIPHERTEXT_BYTES")) fail("checkpoint bounds missing");

if (!server.includes("fn cancel_task_targets(")) fail("shared high-level cancellation helper missing");
if (!server.includes("task_control_cancel_does_not_skip_owned_session_after_active_request_success")) fail("cancel active+owner race regression missing");
const cancelStart = server.indexOf('"cancel" => {');
const cancelEnd = server.indexOf('_ => {', cancelStart);
const cancelBlock = server.slice(cancelStart, cancelEnd);
if (cancelBlock.includes("cancelled == 0")) fail("task_control still conditionally skips current-owner session cancellation");
if (!cancelBlock.includes("task_state.current_owner()") || !cancelBlock.includes("cancel_task_targets(")) fail("task_control cancel does not resolve current owner through Task ownership");

for (const code of ["PolicyDenied","WorkspaceDenied","RuntimeUnavailable","InvalidShellSyntax","PrivilegedRouteUnavailable","ProcessTimedOut","ProcessFailed","ProcessCancelled","SessionUnavailable"]) {
  if (!facade.includes(code)) fail(`canonical typed error missing: ${code}`);
}
for (const status of ["running","completed","failed","cancelled","timed_out","lost"]) {
  if (!facade.includes(`"${status}"`) && !taskState.includes(`"${status}"`)) fail(`public process lifecycle state missing: ${status}`);
}
if (facade.includes('"lost","explained"') || facade.includes('Value::String("explained".into())')) fail("public process lifecycle still exposes a seventh explained state");

for (const marker of [
  '"current_activity"', '"last_activity"', '"waiting"',
  'merge_task_aggregate_activity', 'current_task_activity_value', 'workflow_activity_value', 'command_activity_value',
]) if (!(facade + server).includes(marker)) fail(`schema42 TaskAggregate marker missing: ${marker}`);
if (shell.includes("fn normalize_cmd_reserved_device_redirection") || shell.includes('output.push_str("nul.localbridge")')) fail("cmd NUL device is still rewritten to a workspace file");
if (!shell.includes('ResolvedShellKind::Cmd => "windows_oem"')) fail("cmd output encoding is not bound to Windows OEM semantics");
if (!runtimeProcesses.includes("GetOEMCP") || !runtimeProcesses.includes("decode_output_bytes")) fail("runtime OEM decoder missing");
if (!runtimeServer.includes('env.pop("LOCALBRIDGE_OUTPUT_ENCODING", None)')) fail("internal output encoding marker leaks to child environment");
if (!facade.includes("提高 max_bytes 或 resize 后重试")) fail("OutputTruncated remediation still assumes output_ref exists");
for (const marker of ['format!("directory {}/{}"', 'Some("patch".into())', 'format!("command {}/{}"']) {
  if (!facade.includes(marker)) fail(`generic durable workflow step marker missing: ${marker}`);
}
if (!facade.includes('"Agent workflow context ready"')) fail("context-only diagnose summary is not context-ready");

console.log("SCHEMA42_EXECUTION_PLATFORM=PASS revision=42 task_aggregate=true nul_native=true native_codepage=true durable_steps=true context_ready=true core_tools=8");
