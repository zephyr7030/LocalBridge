import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(process.env.LOCALBRIDGE_REPO_ROOT ?? ".");
if (
  process.env.LOCALBRIDGE_ARCH_RULE_ID !== "ARCH-026"
  || process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR !== "LB-012"
) process.exit(2);

const read = (path) => readFileSync(join(root, path), "utf8");
const requiredFiles = [
  "runtime-policy.toml",
  "src-tauri/src/mcp/policy.rs",
  "src-tauri/src/mcp/server.rs",
  "src-tauri/src/mcp/shell.rs",
  "src-tauri/src/privilege/control.rs",
  "src-tauri/src/privilege/broker.rs",
  "src-tauri/src/privilege/filesystem.rs",
  "tests/integration/privilege/broker_ipc.rs",
];
for (const path of requiredFiles) {
  if (!existsSync(join(root, path))) throw new Error(`ARCH-026 artifact missing: ${path}`);
}

const runtimePolicy = read("runtime-policy.toml");
for (const required of [
  '[administrator_gateway]',
  'route = "broker_only"',
  'token_scope = "administrator_token"',
  'direct_process = "structured_absolute_program_argv"',
  'shell = "trusted_logical_selector_only"',
  'filesystem = "structured_absolute_path_broker"',
  'system_management_identity = "exact_system32"',
  'arbitrary_shell_executable_path = "deny"',
  'control_plane_mutation = "deny_always"',
]) if (!runtimePolicy.includes(required)) throw new Error(`ARCH-026 administrator policy missing: ${required}`);

const policy = read("src-tauri/src/mcp/policy.rs");
for (const required of [
  '"process" => reviewed_administrator_process(arguments)',
  '"shell" => reviewed_administrator_shell(arguments)',
  '"filesystem" => reviewed_administrator_filesystem(arguments)',
  "trusted_system_program(basename)",
  "administrator_shell_executable(basename)",
  "explicit_control_plane_reference",
  '"reg.exe"',
  '"schtasks.exe"',
  '"sc.exe"',
  '"netsh.exe"',
  '"bcdedit.exe"',
  '"dism.exe"',
]) if (!policy.includes(required)) throw new Error(`ARCH-026 policy seam missing: ${required}`);

const server = read("src-tauri/src/mcp/server.rs");
for (const required of [
  '"enum": ["process", "shell", "filesystem"]',
  'Some("process") =>',
  'Some("shell") =>',
  'Some("filesystem") =>',
  "if !matches!(privileged.state(), PrivilegeState::Active { .. })",
  "execution_guard.elevated_decision(mode, &reviewed_arguments)",
  "privileged.start_execute",
  "privileged.poll_execute",
  "privileged.filesystem(spec)",
  "broker_direct_spec(&shell_spec)",
  "typed_administrator_process_shell_and_filesystem_routes_are_broker_only",
]) if (!server.includes(required)) throw new Error(`ARCH-026 Broker route missing: ${required}`);
const elevatedStart=server.indexOf('"name": "elevated_exec"'); const elevatedEnd=server.indexOf("fn elevated_exec_output_schema",elevatedStart); if(elevatedStart<0||elevatedEnd<=elevatedStart||server.slice(elevatedStart,elevatedEnd).includes('"oneOf"')) throw new Error("ARCH-026 elevated_exec input schema regressed to a client-hostile top-level combinator");
if (server.slice(server.indexOf("fn handle_elevated_exec"), server.indexOf("fn request_id", server.indexOf("fn handle_elevated_exec"))).includes("guard.call_tool")) {
  throw new Error("ARCH-026 privileged administrator route reaches ordinary upstream MCP");
}

const shell = read("src-tauri/src/mcp/shell.rs");
for (const required of [
  "pub fn resolve_for_broker",
  "fn highest_core_for_broker",
  "pub fn broker_direct_spec",
  "broker_shell_preparation_never_runs_version_probe_under_ordinary_token",
]) if (!shell.includes(required)) throw new Error(`ARCH-026 trusted logical shell seam missing: ${required}`);
const brokerResolverStart = shell.indexOf("pub fn resolve_for_broker");
const ordinaryHighestCoreStart = shell.indexOf("fn highest_core(", brokerResolverStart);
if (brokerResolverStart < 0 || ordinaryHighestCoreStart <= brokerResolverStart) {
  throw new Error("ARCH-026 Broker shell resolver boundary missing");
}
if (shell.slice(brokerResolverStart, ordinaryHighestCoreStart).includes("probe_powershell_core")) {
  throw new Error("ARCH-026 Broker shell resolution probes a candidate under the ordinary token");
}

const control = read("src-tauri/src/privilege/control.rs");
for (const method of ["start_execute", "poll_execute", "cancel_execute", "filesystem"]) {
  const implStart = control.indexOf("impl PrivilegedExecution for PrivilegedExecutionGateway");
  const start = control.indexOf(`fn ${method}`, implStart);
  if (start < 0 || !control.slice(start, start + 850).includes("require_gate()?")) {
    throw new Error(`ARCH-026 ${method} does not re-check Active Broker gate`);
  }
}

const filesystem = read("src-tauri/src/privilege/filesystem.rs");
if (/Command::new|std::process::Command|powershell\.exe|cmd\.exe/i.test(filesystem)) {
  throw new Error("ARCH-026 privileged filesystem is shell/process-backed");
}
const brokerTests = read("tests/integration/privilege/broker_ipc.rs");
for (const required of [
  "actual_broker_structured_execution_supports_completion_timeout_cancel_limit_and_redaction",
  "actual_broker_structured_filesystem_roundtrips_outside_workspace_without_shell",
]) if (!brokerTests.includes(required)) throw new Error(`ARCH-026 behavioral evidence missing: ${required}`);

console.log("ARCH-026_VERIFY=PASS broker_only=true general_admin_process=true trusted_logical_shell=true privileged_filesystem=true system32_management_identity=true control_plane_denied=true active_gate=true");
