import { readFileSync } from "node:fs";

const ruleId = process.env.LOCALBRIDGE_ARCH_RULE_ID;
if (ruleId && ruleId !== "ARCH-031") throw new Error(`ARCH-031 invoked as ${ruleId}`);

const contracts = JSON.parse(readFileSync("PR_CONTRACTS.json", "utf8"));
if (contracts.rules?.dynamic_privileged_tool_catalog_refresh_required !== false) throw new Error("ARCH-031 dynamic privileged catalog refresh must be disabled");
for (const key of [
  "privileged_tool_stable_advertisement_required",
  "elevated_exec_advertised_in_all_permission_modes",
  "elevated_exec_advertisement_broker_state_independent",
  "elevated_exec_call_time_authorization_required",
]) if (contracts.rules?.[key] !== true) throw new Error(`ARCH-031 missing rule ${key}`);

const lb007 = contracts.prs?.["LB-007"];
const lb012 = contracts.prs?.["LB-012"];
const lb007Proof = "elevated_exec remains advertised in Edit Full and Elevated and across Broker Disabled Requested AwaitingUac Active or Faulted states; catalog visibility never grants authority, every tools/call is re-authorized against current PermissionMode and Broker state, Edit/Full return typed PrivilegedRouteNotAvailable, Elevated without Active Broker returns typed ElevationRequired, and Full<->Elevated or Broker-state changes do not require reconnect solely for elevated_exec visibility";
const lb012Proof = "an already-connected MCP session sees elevated_exec continuously before and after entering Elevated or Broker activation; Full and Edit calls remain typed denied, Elevated without Active Broker remains awaiting authorization, Active Broker enables only reviewed administrator operations, and ordinary exec_command remains current-user throughout";
if (!lb007?.required_tests?.includes(lb007Proof)) throw new Error("ARCH-031 LB-007 stable advertisement proof missing");
if (!lb012?.required_tests?.includes(lb012Proof)) throw new Error("ARCH-031 LB-012 stable Broker-gated proof missing");

const facade = readFileSync("src-tauri/src/mcp/facade.rs", "utf8");
const policy = readFileSync("src-tauri/src/mcp/policy.rs", "utf8");
const server = readFileSync("src-tauri/src/mcp/server.rs", "utf8");
const runtimePolicy = readFileSync("runtime-policy.toml", "utf8");
if (!facade.includes("pub const AGENT_API_REVISION: u32 = 36")) throw new Error("ARCH-031 facade revision36 missing");
if (!policy.includes("pub fn privileged_tool_visible(&self, _mode: PermissionMode, tool_name: &str) -> bool")) throw new Error("ARCH-031 privileged visibility is still mode-dependent");
const catalogStart = server.indexOf("fn effective_tool_catalog(");
const signatureStart = server.indexOf("fn effective_tool_catalog_signature(");
const catalogSlice = server.slice(catalogStart, signatureStart);
if (!catalogSlice.includes("append_elevated_exec_tool(&mut result)")) throw new Error("ARCH-031 elevated_exec not appended to effective catalog");
if (catalogSlice.includes("accepts_privileged_calls")) throw new Error("ARCH-031 catalog still depends on Broker Active state");
const signatureEnd = server.indexOf("fn elevation_required_result", signatureStart);
const signatureSlice = server.slice(signatureStart, signatureEnd);
if (!signatureSlice.includes('names.push("elevated_exec")')) throw new Error("ARCH-031 catalog signature omits stable elevated_exec");
if (signatureSlice.includes("accepts_privileged_calls")) throw new Error("ARCH-031 signature still depends on Broker state");
const setterStart = server.indexOf("pub fn set_permission_mode(&self, mode: PermissionMode)");
const setterEnd = server.indexOf("pub fn replace_policy", setterStart);
if (server.slice(setterStart, setterEnd).includes("sessions.clear")) throw new Error("ARCH-031 permission mode still unconditionally clears MCP sessions");
for (const marker of [
  'assert_tool_error(&full_denied, "PrivilegedRouteUnavailable")',
  'assert_tool_error(&edit_denied, "PrivilegedRouteUnavailable")',
  'assert_tool_error(&awaiting, "ElevationRequired")',
]) if (!server.includes(marker)) throw new Error(`ARCH-031 call-time denial proof missing: ${marker}`);
if (!runtimePolicy.includes('elevated_exec_in_edit = "deny"') || !runtimePolicy.includes('elevated_exec_in_full = "deny"') || !runtimePolicy.includes('elevated_exec_in_elevated = "allow_if_reviewed_and_broker_active"')) throw new Error("ARCH-031 runtime policy call-time matrix drift");
console.log("ARCH-031_VERIFY=PASS elevated_exec_always_advertised=true broker_state_independent_catalog=true call_time_policy=true edit_full_typed_denied=true elevated_requires_active_broker=true");
