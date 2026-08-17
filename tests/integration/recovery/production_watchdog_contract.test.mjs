import { readFileSync } from "node:fs";

const read = (path) => readFileSync(path, "utf8");
const background = read("src-tauri/src/app/background.rs");
const recovery = read("src-tauri/src/runtime/recovery.rs");
const orchestrator = read("src-tauri/src/runtime/orchestrator.rs");
const guard = read("src-tauri/src/mcp/guard.rs");
const server = read("src-tauri/src/mcp/server.rs");
const mcpRuntime = read("src-tauri/src/mcp/runtime.rs");
const mcpHttp = read("src-tauri/src/mcp/http.rs");
const tunnelRuntime = read("src-tauri/src/tunnel/runtime.rs");
const tunnelHealth = read("src-tauri/src/tunnel/health.rs");

for (const required of [
  'name("localbridge-runtime-watchdog"',
  "recv_timeout(RUNTIME_WATCHDOG_INTERVAL)",
  "monitor_operation.try_lock()",
  "runtime.monitor_recovery()",
  "AutoRecoveryRuntime::new_with_cancellation(",
  "recovery_cancellation: RecoveryCancellation",
  "runtime_snapshot_cache: Arc<RwLock<DesktopRuntimeSnapshot>>",
  "impl Drop for DesktopLifecycle",
]) {
  if (!background.includes(required)) throw new Error(`LB-010 production watchdog wiring missing: ${required}`);
}

if (!recovery.includes("self.runtime.probe_ready_health()")) throw new Error("LB-010 AutoRecoveryRuntime does not probe post-Ready health");
const monitorStart = recovery.indexOf("pub fn monitor_once");
const monitorEnd = recovery.indexOf("pub fn manual_retry_current_outage", monitorStart);
const monitor = recovery.slice(monitorStart, monitorEnd);
if (monitorStart < 0 || monitorEnd <= monitorStart) throw new Error("LB-010 cooperative monitor method missing");
if (monitor.includes("recover_auto(")) throw new Error("LB-010 production monitor regressed to synchronous five-attempt recovery");
for (const required of ["advance_pending_auto", "begin_cooperative_auto", "pending_auto"]) {
  if (!monitor.includes(required)) throw new Error(`LB-010 cooperative monitor missing: ${required}`);
}
if (!recovery.includes("self.controller.observe_stable_ready")) throw new Error("LB-010 production monitor no longer observes 60s stable Ready");
if (!recovery.includes("manual_retry_current_outage")) throw new Error("LB-010 persistent manual retry path missing");
for (const required of ["PendingAutoRecovery", "next_deadline", "recover_minimal_cancellable", "RecoveryCancellation", "RecoveryPermit"]) {
  if (!recovery.includes(required) && !orchestrator.includes(required)) throw new Error(`LB-010 cooperative recovery primitive missing: ${required}`);
}
for (const required of [
  "set_permission_mode_after_control_cancellation",
  "resume_after_control_interruption",
  "pending.permit = fresh_permit",
  "next_attempt: attempt",
]) if (!recovery.includes(required)) throw new Error(`LB-010 control-interrupted recovery reconciliation missing: ${required}`);

const autoRuntimeImplStart = background.indexOf("impl<D, C> ExitRuntime for AutoRecoveryRuntime");
const autoPermissionStart = background.indexOf("fn set_permission_mode", autoRuntimeImplStart);
const autoWorkspaceStart = background.indexOf("fn switch_workspace", autoPermissionStart);
if (autoRuntimeImplStart < 0 || autoPermissionStart < 0 || autoWorkspaceStart <= autoPermissionStart) throw new Error("LB-010 AutoRecoveryRuntime production permission adapter missing");
const autoPermission = background.slice(autoPermissionStart, autoWorkspaceStart);
if (!autoPermission.includes("set_permission_mode_after_control_cancellation(mode)")) throw new Error("LB-010 production permission switch bypasses recovery reconciliation");
if (autoPermission.includes("orchestrator_mut()")) throw new Error("LB-010 production permission switch directly mutates orchestrator during cancelled recovery");

const functionBody = (source, name, privateMethod = false) => {
  const marker = `${privateMethod ? "fn" : "pub fn"} ${name}`;
  const start = source.indexOf(marker);
  if (start < 0) throw new Error(`LB-010 lifecycle function missing: ${name}`);
  const nextPub = source.indexOf("\n    pub fn ", start + marker.length);
  const nextPrivate = source.indexOf("\n    fn ", start + marker.length);
  const candidates = [nextPub, nextPrivate].filter((index) => index > start);
  const end = candidates.length ? Math.min(...candidates) : source.length;
  return source.slice(start, end);
};

for (const [name, privateMethod] of [
  ["stop_services_for_manual_action", false],
  ["stop_runtime_for_control_plane", false],
  ["set_runtime_permission_mode", false],
  ["switch_runtime_workspace", false],
  ["manual_retry_after_attention", false],
  ["shutdown_with_privilege", true],
]) {
  const body = functionBody(background, name, privateMethod);
  const compact = body.replace(/\s+/g, "");
  const cancel = compact.indexOf("self.recovery_cancellation.cancel()");
  const operationLock = compact.indexOf("self.runtime_operation");
  if (!(cancel >= 0 && operationLock > cancel)) {
    throw new Error(`LB-010 explicit control does not cancel recovery before lifecycle lock: ${name}`);
  }
}

const snapshot = functionBody(background, "runtime_snapshot");
if (!snapshot.includes("runtime_snapshot_cache") || !snapshot.includes(".read()")) throw new Error("LB-010 runtime_snapshot does not use independent cache");
if (snapshot.includes("self.runtime\n") || snapshot.includes(".lock()")) throw new Error("LB-010 runtime_snapshot can still block on runtime owner");

for (const required of ["probe_mcp_health", "probe_pep_health", "probe_tunnel_health", "probe_ready_health"]) {
  if (!orchestrator.includes(required)) throw new Error(`LB-010 production health probe missing: ${required}`);
}
for (const required of ["take_coding_runtime_fault()", "coding_runtime_health()", "CodingRuntimeHealthState::Ready", "health.authenticated_mcp", "RuntimeFault::McpHealthTimeout"]) {
  if (!orchestrator.includes(required)) throw new Error(`LB-010 authenticated MCP watchdog bridge missing: ${required}`);
}
if (!orchestrator.includes("wait_ready_for_recovery(Duration::ZERO, Duration::from_millis(250), || false)")) throw new Error("LB-010 Tunnel watchdog probe is not transport-bounded");
if (!guard.includes("runtime_root_is_running")) throw new Error("LB-010 McpGuard liveness seam missing");
if (!server.includes("coding_runtime_health") || !server.includes("take_coding_runtime_fault")) throw new Error("LB-010 PEP authenticated MCP health/fault accessors missing");
if (!server.includes("TryLockError::WouldBlock") || !server.includes("Ok(None)")) throw new Error("LB-010 busy MCP guard must defer health judgment instead of causing a false outage");

for (const required of ["start_for_recovery", "wait_ready_for_recovery", "initialize_with_timeout", "post_json_with_timeouts"]) {
  if (!mcpRuntime.includes(required) && !mcpHttp.includes(required)) throw new Error(`LB-010 bounded MCP recovery seam missing: ${required}`);
}
if (!orchestrator.includes("Duration::from_millis(250)")) throw new Error("LB-010 production recovery transport slice is not bounded to 250ms");
for (const required of ["wait_ready_for_recovery", "probe_ready_metadata_with_timeout"]) {
  if (!tunnelRuntime.includes(required) && !tunnelHealth.includes(required)) throw new Error(`LB-010 bounded Tunnel recovery seam missing: ${required}`);
}
for (const endpoint of ['"/readyz"', '"/api/status"']) {
  if (!tunnelHealth.includes(endpoint)) throw new Error(`LB-010 Tunnel recovery health authentication surface missing: ${endpoint}`);
}
if (!mcpHttp.includes("Duration::from_millis(500)") || !mcpHttp.includes("Duration::from_secs(2)")) throw new Error("LB-010 ordinary MCP transport timeouts were not preserved");
if (!tunnelHealth.includes("Duration::from_millis(500)") || !tunnelHealth.includes("Duration::from_secs(2)")) throw new Error("LB-010 ordinary Tunnel health timeouts were not preserved");

console.log("LB010_PRODUCTION_WATCHDOG=PASS cooperative_auto=true cancellable=true permission_interrupt_resume=true snapshot_cache=true mcp_probe=true pep_probe=true tunnel_probe=true stable_reset=true single_owner=true");
