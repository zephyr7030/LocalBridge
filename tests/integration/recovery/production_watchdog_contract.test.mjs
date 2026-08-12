import { readFileSync } from "node:fs";

const read = (path) => readFileSync(path, "utf8");
const background = read("src-tauri/src/app/background.rs");
const recovery = read("src-tauri/src/runtime/recovery.rs");
const orchestrator = read("src-tauri/src/runtime/orchestrator.rs");
const guard = read("src-tauri/src/mcp/guard.rs");
const server = read("src-tauri/src/mcp/server.rs");

for (const required of [
  'name("localbridge-runtime-watchdog"',
  "recv_timeout(RUNTIME_WATCHDOG_INTERVAL)",
  "monitor_operation.try_lock()",
  "runtime.monitor_recovery()",
  "AutoRecoveryRuntime::new(runtime, SystemRecoveryClock::default())",
  "impl Drop for DesktopLifecycle",
]) {
  if (!background.includes(required)) throw new Error(`LB-010 production watchdog wiring missing: ${required}`);
}

if (!recovery.includes("self.runtime.probe_ready_health()")) throw new Error("LB-010 AutoRecoveryRuntime does not probe post-Ready health");
if (!recovery.includes("self.controller.recover_auto")) throw new Error("LB-010 production monitor no longer owns automatic recovery");
if (!recovery.includes("self.controller.observe_stable_ready")) throw new Error("LB-010 production monitor no longer observes 60s stable Ready");
if (!recovery.includes("manual_retry_current_outage")) throw new Error("LB-010 persistent manual retry path missing");

for (const required of ["probe_mcp_health", "probe_pep_health", "probe_tunnel_health", "probe_ready_health"]) {
  if (!orchestrator.includes(required)) throw new Error(`LB-010 production health probe missing: ${required}`);
}
if (!orchestrator.includes("upstream_root_is_running()")) throw new Error("LB-010 MCP liveness is not checked through PEP");
if (!orchestrator.includes("wait_ready(Duration::ZERO)")) throw new Error("LB-010 Tunnel watchdog probe is not one-shot/nonblocking");
if (!guard.includes("runtime_root_is_running")) throw new Error("LB-010 McpGuard liveness seam missing");
if (!server.includes("upstream_root_is_running")) throw new Error("LB-010 PEP upstream MCP liveness accessor missing");
if (!server.includes("TryLockError::WouldBlock") || !server.includes("Ok(None)")) throw new Error("LB-010 busy MCP guard must defer health judgment instead of causing a false outage");

console.log("LB010_PRODUCTION_WATCHDOG=PASS auto_recovery=true mcp_probe=true pep_probe=true tunnel_probe=true stable_reset=true single_owner=true");
