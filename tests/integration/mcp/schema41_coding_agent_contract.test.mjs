import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const read = (path) => readFileSync(new URL(`../../../${path}`, import.meta.url), "utf8");
const facade = read("src-tauri/src/mcp/facade.rs");
const server = read("src-tauri/src/mcp/server.rs");
const http = read("src-tauri/src/mcp/http.rs");
const policy = read("src-tauri/src/mcp/policy.rs");
const checkpoint = read("src-tauri/src/mcp/workflow_checkpoint.rs");
const context = read("src-tauri/src/mcp/context_service.rs");
const edit = read("src-tauri/src/mcp/edit_service.rs");
const planner = read("src-tauri/src/mcp/verification_planner.rs");

const requireAll = (source, markers, label) => {
  for (const marker of markers) assert.ok(source.includes(marker), `${label} missing ${marker}`);
};

assert.ok(facade.includes("pub const AGENT_API_REVISION: u32 = 42;"), "schema42 facade revision drift");
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
requireAll(server, ["task_aggregate_snapshot", "cancel_durable_workflow", "stable_success(data, \"Task control completed\")"], "task_control TaskAggregate integration");
requireAll(policy, ['phase == Some("verify")', "process_exec"], "phase=verify capability policy");
requireAll(http, ["total_timeout: Option<Duration>", "remaining_until(deadline)"], "command-control end-to-end transport deadline");
requireAll(server, ["poll wait_ms budget exceeded", "write wait_ms budget exceeded", "kill wait_ms budget exceeded"], "command-control wall-clock budget regression");
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

const root = fileURLToPath(new URL("../../../", import.meta.url));
const semanticTarget = process.env.CARGO_TARGET_DIR || path.join(root, "src-tauri", "target-schema41-contract");
const semantic = spawnSync(
  "cargo",
  ["test", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--lib", "mcp::", "--", "--nocapture"],
  {
    cwd: root,
    env: { ...process.env, CARGO_TARGET_DIR: semanticTarget },
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
    windowsHide: true,
  },
);
assert.equal(
  semantic.status,
  0,
  "schema42 semantic acceptance failed\nSTDOUT:\n" + semantic.stdout + "\nSTDERR:\n" + semantic.stderr,
);
const semanticLog = semantic.stdout + "\n" + semantic.stderr;
for (const testName of [
  "changed_targeted_scripts_require_a_real_git_changed_set",
  "planner_uses_required_precedence_and_mixed_node_rust_manifests",
  "negative_instruction_code_spans_are_not_execution_requirements",
  "relevance_scoring_finds_deep_runtime_recovery_after_many_noise_files",
  "junction_escape_is_excluded_from_context_discovery",
  "concurrent_existing_writer_handle_fails_closed_before_any_overwrite",
  "concurrent_create_new_has_exactly_one_winner_and_never_overwrites",
  "schema41_phased_coding_task_rejects_skipped_verify_or_persist",
  "durable_checkpoint_omits_stdin_while_initial_execution_still_receives_it",
  "schema40_health_probe_remains_authenticated_while_facade_lock_is_held",
  "schema40_real_root_process_alive_but_mcp_unresponsive_is_not_ready",
  "schema41_incomplete_checkpoint_cannot_be_overwritten_by_unrelated_workflow",
  "schema41_resume_rejects_changed_git_head_before_side_effects",
  "schema41_workspace_context_projects_durable_task_truth",
  "schema41_stderr_pages_share_one_sanitized_public_byte_space",
  "schema41_private_patch_errors_keep_canonical_conflict_codes",
  "schema41_document_create_existing_target_returns_file_changed",
  "schema41_workflow_workdir_and_wait_budget_are_discoverable",
  "schema42_task_aggregate_waiting_is_not_idle",
  "schema42_command_summary_is_status_derived",
  "schema42_document_eof_is_not_truncation",
  "schema42_command_kill_leaves_workflow_waiting",
  "task_control_cancel_reaches_running_call_without_waiting_for_facade_execution_lock",
  "task_control_cancel_owns_detached_public_command_session",
  "durable_task_terminal_ignores_newer_unrelated_command",
  "schema28_public_runtime_behavior_is_real_end_to_end",
]) {
  assert.ok(semanticLog.includes(testName + " ... ok"), "semantic proof did not execute " + testName);
}

console.log(JSON.stringify({ profile: "coding-agent-v1", core_tools: 8, capabilities: matrix, semantic_acceptance: "PASS" }, null, 2));
