import { readFileSync } from "node:fs";

const source = readFileSync("src-tauri/src/runtime/orchestrator.rs", "utf8");
const required = [
  "pub trait RuntimeDriver",
  "RuntimeState::StartingMcp",
  "RuntimeState::WaitingMcpReady",
  "RuntimeState::StartingPolicyEnforcement",
  "RuntimeState::WaitingPolicyReady",
  "RuntimeState::StartingTunnel",
  "RuntimeState::WaitingTunnelReady",
  "RuntimeState::Ready",
  "CodingToolsRuntime::start",
  "CodingToolsPermissionMode::Trusted",
  "PolicyEnforcementRuntime::start",
  "PreparedTunnelStart::prepare",
  "PreparedTunnelStart::spawn",
  "pep.port()",
  "self.driver.stop_tunnel",
  "self.driver.stop_pep",
  "self.driver.stop_mcp",
  "pub fn current_task(&self) -> CurrentTaskStatus",
  "pub struct OutageTracker",
  "mark_user_attention_required",
];
for (const text of required) {
  if (!source.includes(text)) throw new Error(`LB-009 orchestrator contract missing: ${text}`);
}
for (const forbidden of [
  "[1, 2, 5, 10, 30]",
  "Duration::from_secs(1)",
  "SwitchingWorkspace",
  "WorkspaceRegistry",
  "Vec<CurrentTaskStatus>",
  "Vec<RuntimeState>",
  "activity_history",
  "task_history",
]) {
  if (source.includes(forbidden)) throw new Error(`LB-009 implements forbidden/non-goal behavior: ${forbidden}`);
}
const stopTunnel = source.indexOf("self.driver.stop_tunnel");
const stopPep = source.indexOf("self.driver.stop_pep", stopTunnel);
const stopMcp = source.indexOf("self.driver.stop_mcp", stopPep);
if (!(stopTunnel >= 0 && stopPep > stopTunnel && stopMcp > stopPep)) {
  throw new Error("LB-009 reverse shutdown order is not Tunnel -> PEP -> MCP");
}
console.log("LB009_ORCHESTRATOR_CONTRACT=PASS staged_start=true reverse_stop=true live_task_only=true outage_generation_only=true no_backoff=true");
