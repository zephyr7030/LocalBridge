import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(process.env.LOCALBRIDGE_REPO_ROOT ?? ".");
if (
  process.env.LOCALBRIDGE_ARCH_RULE_ID !== "ARCH-027"
  || process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR !== "LB-012"
) process.exit(2);

const read = (path) => readFileSync(join(root, path), "utf8");
for (const path of [
  "src-tauri/src/mcp/path_authority.rs",
  "src-tauri/src/mcp/facade.rs",
  "src-tauri/src/mcp/policy.rs",
  "src-tauri/src/mcp/server.rs",
  "src-tauri/src/privilege/control.rs",
  "src-tauri/src/privilege/protocol.rs",
  "tests/integration/privilege/broker_ipc.rs",
]) if (!existsSync(join(root, path))) throw new Error(`ARCH-027 artifact missing: ${path}`);

const authority = read("src-tauri/src/mcp/path_authority.rs");
for (const required of [
  "PathAuthorityScope::ActiveWorkspace",
  "PathAuthorityScope::BrokerAdministrator",
  "workspace_input_path_valid(raw)",
  "workspace_absolute_path_valid",
  "administrator_absolute_path_valid(raw)",
  "PathAuthorityScope::ActiveWorkspace => canonical.starts_with",
  "PathAuthorityScope::BrokerAdministrator => canonical.is_absolute()",
  "active_workspace_accepts_relative_or_absolute_inside_and_is_canonical_root_bound",
  "broker_administrator_accepts_only_ordinary_absolute_dispatch_paths",
]) if (!authority.includes(required)) throw new Error(`ARCH-027 authority seam missing: ${required}`);

const facade = read("src-tauri/src/mcp/facade.rs");
for (const required of [
  "PathAuthority::active_workspace(&self.workspace)",
  "workspace_input_path_valid",
  "normalized_workspace_path",
]) if (!facade.includes(required)) throw new Error(`ARCH-027 ordinary facade authority missing: ${required}`);
if (facade.includes("PathAuthority::broker_administrator()")) {
  throw new Error("ARCH-027 ordinary public facade constructs BrokerAdministrator authority");
}

const policy = read("src-tauri/src/mcp/policy.rs");
for (const required of [
  "mode != PermissionMode::Elevated",
  'tool_name == "elevated_exec"',
  "DenyReason::PrivilegedRouteNotAvailable",
  "ordinary_system_management_targets_require_the_privileged_route_in_full_and_elevated",
  "schema33_bcdedit_and_dism_require_privileged_route_in_direct_and_workflow_paths",
]) if (!policy.includes(required) && !read("tests/integration/policy/policy_enforcement.rs").includes(required)) {
  throw new Error(`ARCH-027 mode-boundary evidence missing: ${required}`);
}

const server = read("src-tauri/src/mcp/server.rs");
if (!server.includes("if !matches!(privileged.state(), PrivilegeState::Active { .. })")) {
  throw new Error("ARCH-027 elevated_exec execution is not tied to Active Broker state");
}
if (!server.includes("privileged.filesystem(spec)")) {
  throw new Error("ARCH-027 outside-workspace filesystem route is not Broker-backed");
}

const control = read("src-tauri/src/privilege/control.rs");
const fsImplStart = control.indexOf("fn filesystem", control.indexOf("impl PrivilegedExecution for PrivilegedExecutionGateway"));
if (fsImplStart < 0 || !control.slice(fsImplStart, fsImplStart + 700).includes("require_gate()?")) {
  throw new Error("ARCH-027 privileged filesystem does not require an Active Broker gate");
}

const protocol = read("src-tauri/src/privilege/protocol.rs");
for (const required of [
  "pub struct PrivilegedFilesystemSpec",
  "valid_privileged_absolute_path(&self.path)",
  "Path::new(value)",
  "path.is_absolute()",
]) if (!protocol.includes(required)) throw new Error(`ARCH-027 privileged absolute-path contract missing: ${required}`);

const brokerTests = read("tests/integration/privilege/broker_ipc.rs");
if (!brokerTests.includes("actual_broker_structured_filesystem_roundtrips_outside_workspace_without_shell")) {
  throw new Error("ARCH-027 outside-workspace Broker filesystem behavioral evidence missing");
}

console.log("ARCH-027_VERIFY=PASS structured_localbridge_paths_active_workspace=true ordinary_route_no_broker_authority=true full_process_token=current_windows_user_contract full_os_workspace_sandbox_guaranteed=false elevated_active_broker=true administrator_absolute_scope=true administrator_route_broker_only=true");
