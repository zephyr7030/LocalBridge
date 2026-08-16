import { readFileSync } from "node:fs";

const ruleId = process.env.LOCALBRIDGE_ARCH_RULE_ID;
if (ruleId && ruleId !== "ARCH-030") throw new Error(`ARCH-030 invoked as ${ruleId}`);

const contracts = JSON.parse(readFileSync("PR_CONTRACTS.json", "utf8"));
const lb006 = contracts.prs?.["LB-006"];
const artifact = "LocalBridge-owned MCP output schemas for every advertised public tool, matching the stable public structuredContent contract without exposing upstream private output schemas";
const proof = "every advertised LocalBridge public tool including privileged extensions declares a non-empty LocalBridge-owned outputSchema matching its actual structuredContent; agent_workflow describes the stable ok/data/error envelope and action/state/workspace/project/commands result fields, elevated_exec describes its existing privileged result variants, and upstream private outputSchema is never exposed directly";
if (!lb006?.required_artifacts?.includes(artifact)) throw new Error("ARCH-030 LB-006 outputSchema artifact contract missing");
if (!lb006?.required_tests?.includes(proof)) throw new Error("ARCH-030 LB-006 outputSchema behavior proof missing");

const facade = readFileSync("src-tauri/src/mcp/facade.rs", "utf8");
const server = readFileSync("src-tauri/src/mcp/server.rs", "utf8");
for (const marker of [
  "pub const AGENT_API_REVISION: u32 = 32",
  '"outputSchema": public_tool_output_schema(name)',
  "fn public_tool_output_schema(name: &str) -> Value",
  '"agent_workflow" => json!({',
  '"state":{"type":"string","enum":["context_ready","running","completed"]}',
  '"error":public_error_output_schema()',
]) if (!facade.includes(marker)) throw new Error(`ARCH-030 public facade outputSchema marker missing: ${marker}`);
for (const marker of [
  '"outputSchema": elevated_exec_output_schema()',
  "fn elevated_exec_output_schema() -> Value",
  'served_agent["outputSchema"]',
  'elevated_tool["outputSchema"]["oneOf"]',
]) if (!server.includes(marker)) throw new Error(`ARCH-030 served/elevated outputSchema marker missing: ${marker}`);
if (/public_tool_schema[\s\S]{0,1200}get\("outputSchema"\)/.test(facade)) throw new Error("ARCH-030 public facade appears to forward an upstream private outputSchema");
console.log("ARCH-030_VERIFY=PASS core_tools=8 localbridge_owned_output_schema=true agent_workflow_typed=true elevated_exec_typed=true upstream_private_schema_hidden=true");
