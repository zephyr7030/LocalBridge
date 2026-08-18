import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (path) => readFileSync(new URL(`../../../${path}`, import.meta.url), "utf8");
const facade = read("src-tauri/src/mcp/facade.rs");
const server = read("src-tauri/src/mcp/server.rs");
const policy = read("src-tauri/src/mcp/policy.rs");
const checkpoint = read("src-tauri/src/mcp/workflow_checkpoint.rs");
const context = read("src-tauri/src/mcp/context_service.rs");
const edit = read("src-tauri/src/mcp/edit_service.rs");
const planner = read("src-tauri/src/mcp/verification_planner.rs");

const requireAll = (source, markers, label) => {
  for (const marker of markers) assert.ok(source.includes(marker), `${label} missing ${marker}`);
};

assert.ok(facade.includes("pub const AGENT_API_REVISION: u32 = 41;"), "schema41 facade revision drift");
requireAll(facade, [
  'pub const V1_CORE_TOOL_NAMES: [&str; 8]',
  '"workspace_context"', '"agent_workflow"', '"exec_command"', '"command_control"',
  '"git_workflow"', '"document_workflow"', '"view_image"', '"task_control"',
  '"prepare"', '"edit"', '"verify"', '"persist"', '"coding-agent-v1"',
  'FacadeErrorCode::FileChanged', 'FacadeErrorCode::PatchConflict', 'FacadeErrorCode::AmbiguousMatch',
  '"ok","state","summary","task_id","warnings","next_step","output_refs","data","error"'
], "AgentFacade");

requireAll(checkpoint, [
  "const CHECKPOINT_VERSION: u32 = 2;", "pub objective: Option<String>",
  "pub current_step: Option<String>", "pub next_step: Option<String>", "pub files_read: Vec<Value>",
  "pub modified_files: Vec<String>", "pub commands: Vec<Value>", "pub test_results: Vec<Value>",
  "pub build_results: Vec<Value>", "pub failure: Option<Value>", "pub output_refs: Vec<String>",
  "pub git_before: Option<Value>", "pub git_after: Option<Value>"
], "durable coding Task checkpoint");
requireAll(context, ["struct ContextService", "discover_instructions", "search_text", "select_related_files", "read_relevant_ranges"], "ContextService");
requireAll(edit, ["struct CodingEditService", "apply_patch", "FileChanged", "PatchConflict", "AmbiguousMatch", "atomic"], "CodingEditService");
requireAll(planner, ["struct VerificationPlanner", "priority", "source", "plan"], "VerificationPlanner");
requireAll(server, ["durable_coding_task_snapshot", "cancel_durable_coding_task", "stable_success(data, \"Task control completed\")"], "task_control durable Task integration");
requireAll(policy, ['phase == Some("verify")', "process_exec"], "phase=verify capability policy");
requireAll(facade, ["output_ref", "command_control", "resume_coding_task", "coding_verification_plan", "apply_coding_patch", "schema41 stale-projection compatibility", "expected_files_from_checkpoint", "schema41_stale_schema39_client_can_complete_durable_coding_task", "schema41_stale_schema39_resume_rechecks_verify_policy_in_edit_mode"], "coding capability reachability");

const matrix = {
  workspace_discovery: "workspace_context + ContextService",
  project_instructions: "ContextService.discover_instructions",
  context_search: "ContextService.search_text/select_related_files/read_relevant_ranges",
  command_execution: "exec_command/shared ShellExecutor",
  persistent_task: "WorkflowCheckpoint v2",
  resume: "agent_workflow resume_coding_task",
  patch_edit: "CodingEditService.apply_patch",
  test_build: "VerificationPlanner + phase=verify",
  git_status_diff: "git_workflow + git_before/git_after",
  cancellation: "task_control durable/owned-session cancellation",
  output_continuation: "command_control output_ref paging",
  typed_errors: "canonical FacadeErrorCode including edit conflicts"
};
assert.equal(Object.keys(matrix).length, 12);
console.log(JSON.stringify({ profile: "coding-agent-v1", core_tools: 8, capabilities: matrix }, null, 2));
