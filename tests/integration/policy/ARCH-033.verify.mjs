import { readFileSync } from "node:fs";

const ruleId = process.env.LOCALBRIDGE_ARCH_RULE_ID;
if (ruleId && ruleId !== "ARCH-033") throw new Error(`ARCH-033 invoked as ${ruleId}`);
const contracts = JSON.parse(readFileSync("PR_CONTRACTS.json", "utf8"));
for (const key of ["agent_workflow_request_derived_capability_required", "ordinary_full_file_cleanup_not_privileged_by_surface_syntax"]) if (contracts.rules?.[key] !== true) throw new Error(`ARCH-033 missing contract rule ${key}`);
const lb007 = contracts.prs?.["LB-007"];
for (const fragment of ["derived from the actual request contents rather than the action label", "literal cmd call of a workspace .cmd or .bat target", "Python os.remove may complete the create-test-cleanup loop"]) if (!lb007?.required_tests?.some((value) => value.includes(fragment))) throw new Error(`ARCH-033 LB-007 contract missing: ${fragment}`);
const policy = readFileSync("src-tauri/src/mcp/policy.rs", "utf8");
const test = readFileSync("tests/integration/policy/policy_enforcement.rs", "utf8");
for (const marker of ["patch_present || directory_changes_present", "commands_present", "shell_review_required", "eq_ignore_ascii_case(\"call\")"]) if (!policy.includes(marker)) throw new Error(`ARCH-033 request-derived policy marker missing: ${marker}`);
if (/\|\s*"del"\s*\n|\|\s*"erase"\s*\n/.test(policy)) throw new Error("ARCH-033 ordinary cmd file deletion is still globally privileged by keyword");
if (/Some\("ps1"\s*\|\s*"psm1"[\s\S]{0,100}"cmd"/.test(policy)) throw new Error("ARCH-033 script filename extensions are still globally privileged");
for (const marker of ["full_ordinary_workflow_scripts_and_file_cleanup_do_not_require_privileged_route", "call test\\\\lb_broad_tmp.cmd", "del /q test\\\\lb_broad_tmp.cmd", "os.remove", "call %SCRIPT%", "HKLM:"]) if (!test.includes(marker)) throw new Error(`ARCH-033 regression marker missing: ${marker}`);
console.log("ARCH-033_VERIFY=PASS request_derived_workflow=true literal_workspace_scripts=true ordinary_cleanup=true dynamic_and_provider_mutation_fail_closed=true");
