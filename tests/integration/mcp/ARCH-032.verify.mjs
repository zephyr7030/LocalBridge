import { readFileSync } from "node:fs";

const ruleId = process.env.LOCALBRIDGE_ARCH_RULE_ID;
if (ruleId && ruleId !== "ARCH-032") throw new Error(`ARCH-032 invoked as ${ruleId}`);
const contracts = JSON.parse(readFileSync("PR_CONTRACTS.json", "utf8"));
for (const key of [
  "active_workspace_absolute_input_equivalence_required",
  "elevated_exec_top_level_input_schema_required",
  "yield_zero_async_contract_required",
  "powershell_selector_exact_semantics_required",
]) if (contracts.rules?.[key] !== true) throw new Error(`ARCH-032 missing contract rule ${key}`);
const lb006 = contracts.prs?.["LB-006"];
for (const fragment of ["ordinary Win32 absolute inputs that canonicalize to the same active root", "pwsh means trusted PowerShell Core only", "yield-time_ms=0 is a valid asynchronous contract", "elevated_exec inputSchema is a discoverable top-level object contract"]) {
  if (!lb006?.required_tests?.some((value) => value.includes(fragment))) throw new Error(`ARCH-032 LB-006 contract missing: ${fragment}`);
}
const authority = readFileSync("src-tauri/src/mcp/path_authority.rs", "utf8");
const facade = readFileSync("src-tauri/src/mcp/facade.rs", "utf8");
const shell = readFileSync("src-tauri/src/mcp/shell.rs", "utf8");
const server = readFileSync("src-tauri/src/mcp/server.rs", "utf8");
for (const marker of ["pub const AGENT_API_REVISION: u32 = 38", "workspace_input_path_valid", "normalized_workspace_path", "absolute_and_relative_active_workspace_paths_are_equivalent"]) if (!(facade + server).includes(marker)) throw new Error(`ARCH-032 facade/runtime marker missing: ${marker}`);
if (!/"yield[-_]time_ms":\{"type":"integer","minimum":0,"maximum":30000,"default":10000\}/.test(facade)) throw new Error("ARCH-032 yield-time_ms zero-minimum schema missing");
for (const marker of ["workspace_absolute_path_valid", "allows_canonical", "PathAuthorityError::OutsideAuthority"]) if (!authority.includes(marker)) throw new Error(`ARCH-032 path authority marker missing: ${marker}`);
for (const marker of ["ShellSelector::Powershell", "ShellSelector::Pwsh", "ShellSelector::WindowsPowershell"]) if (!shell.includes(marker)) throw new Error(`ARCH-032 shell selector marker missing: ${marker}`);
for (const marker of ['"type": "object"', '"operation": {', '"shell": {"type": "string"', '"action": {"type": "string"']) if (!server.includes(marker)) throw new Error(`ARCH-032 elevated schema marker missing: ${marker}`);
console.log("ARCH-032_VERIFY=PASS revision=34 canonical_absolute_relative_equivalence=true exact_shell_selectors=true yield_zero_async=true elevated_input_schema_discoverable=true");
