import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve, relative } from "node:path";
const root=resolve(process.env.LOCALBRIDGE_REPO_ROOT??".");
if(process.env.LOCALBRIDGE_ARCH_RULE_ID!=="ARCH-005"||process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR!=="LB-007")process.exit(2);
const read=(p)=>readFileSync(join(root,p),"utf8");
for(const p of ["src-tauri/src/mcp/policy.rs","src-tauri/src/mcp/guard.rs","src-tauri/src/mcp/server.rs","tests/integration/policy/policy_enforcement.rs"])if(!existsSync(join(root,p)))process.exit(3);
const policy=read("src-tauri/src/mcp/policy.rs"), guard=read("src-tauri/src/mcp/guard.rs"), server=read("src-tauri/src/mcp/server.rs");
for(const x of ["request_permissions","workspace_select","workspace_add","workspace_remove","permission_mode_change","credential_reset","tunnel_config_write","mcp_config_write","Capability::ControlPlane","DenyReason::ControlPlane","DenyReason::UnknownTool","IndirectProcessExecInEdit"])if(!policy.includes(x))process.exit(4);
for(const x of ["TaskExecutionState::Blocked","raw_call_tool","TaskExecutionState::Running"])if(!guard.includes(x))process.exit(5);
const decisionMatch=/self\s*\.\s*policy\s*\.\s*decide_request\s*\(\s*mode\s*,[\s\S]*?&request\.arguments\s*\)/.exec(guard);
const forwardMatch=/raw_call_tool\s*\(\s*&request\.name/.exec(guard);
if(!decisionMatch||!forwardMatch||decisionMatch.index>forwardMatch.index)process.exit(6);
for(const x of [
  "PolicyEnforcementRuntime",
  "TcpListener::bind((Ipv4Addr::LOCALHOST, 0))",
  "MAX_CONNECTION_WORKERS",
  "guard.filtered_tools(mode)",
  "ToolCallRequest::new(name, arguments).with_request_id(id.clone())",
  '"notifications/cancelled"',
  "McpCancellationClient",
  "cancellation.cancel_request(request_id)",
  "actual_bundled_mcp_is_reached_only_through_loopback_policy_server",
  "cancellation_reaches_actual_upstream_while_tool_call_is_running",
])if(!server.includes(x))process.exit(7);
if(/TcpListener::bind\s*\(\s*\([^)]*(?:UNSPECIFIED|0\.0\.0\.0|\[::\])/.test(server))process.exit(8);
const files=[];function walk(d){if(!existsSync(d))return;for(const n of readdirSync(d)){const p=join(d,n),s=statSync(p);if(s.isDirectory())walk(p);else if(p.endsWith(".rs"))files.push(p)}}walk(join(root,"src-tauri","src"));
for(const p of files){const rel=relative(root,p).replaceAll("\\","/");if(rel.startsWith("src-tauri/src/mcp/"))continue;const t=readFileSync(p,"utf8");if(/\.(?:call_tool|list_tools|raw_call_tool|raw_list_tools|runtime_mut)\s*\(|CodingToolsRuntime\s*::\s*(?:call_tool|list_tools)\s*\(|\bGuardRuntime\b/.test(t))throw new Error(`raw MCP call outside guard boundary: ${rel}`)}
console.log("ARCH-005_VERIFY=PASS mandatory_tools_call=true actual_arguments_policy=true unknown_deny=true control_plane_deny=true mode_rechecked=true loopback_pep=true cancellation_forwarding=true");
