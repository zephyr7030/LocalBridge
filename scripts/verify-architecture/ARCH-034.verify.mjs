import { readFileSync } from "node:fs";
import { resolve } from "node:path";
const root=process.env.LOCALBRIDGE_REPO_ROOT||resolve(".");
const c=JSON.parse(readFileSync(resolve(root,"PR_CONTRACTS.json"),"utf8"));
const r=c.rules||{};
const fail=[];
const exact={workspace_context_permission_mode_required:true,workspace_context_workspace_scope_required:true,workspace_context_ordinary_route_token_required:true,workspace_context_elevated_route_available_required:true,workspace_context_privilege_state_summary_required:true,workspace_context_shell_discovery_summary_required:true,workspace_context_capability_snapshot_required:true,public_ninth_core_tool_for_schema36_forbidden:true,shell_ordinary_diagnostics_not_privileged_by_surface_tokens:true,shell_windows_native_syntax_compatibility_required:true,shell_cmd_nul_redirection_required:true,shell_bespoke_dsl_forbidden:true,policy_explain_via_existing_tools_required:true,policy_explain_must_not_execute_or_authorize:true,path_authority_single_localbridge_implementation_required:true,dashboard_permission_mode_row_forbidden:false,dashboard_permission_mode_read_only_row_required:true,dashboard_permission_mode_controls_forbidden:true,dashboard_admin_privilege_status_read_only:false,admin_consent_backend_challenge_not_before_required:true};
if(c.schema_version!==38)fail.push("schema");
for(const [k,v] of Object.entries(exact))if(r[k]!==v)fail.push(k);
for(const [k,v] of Object.entries({workspace_bound_public_path_inputs_must_resolve_within_active_workspace:true,workspace_safe_ordinary_win32_absolute_input_allowed:true,workspace_safe_relative_and_absolute_input_equivalent_after_identity_validation:true,workspace_public_outside_root_absolute_input_forbidden:true,workspace_public_unc_verbatim_posix_ads_input_forbidden:true,workspace_public_reparse_escape_forbidden:true,agent_workflow_workspace_bound_project_path_selector_required:true,schema37_contract_ratified:true}))if(r[k]!==v)fail.push(k);
for(const [k,v] of Object.entries({task_control_cancel_detached_public_session_required:true,task_control_cancel_shared_public_session_terminator_required:true,git_file_metadata_machine_readable_nul_required:true,git_patch_body_and_file_metadata_independent_required:true,command_control_top_level_discoverable_schema_required:true,elevated_exec_top_level_discoverable_schema_required:true,client_hostile_top_level_input_combinator_for_command_and_elevated_forbidden:true,document_rebuild_schema_existing_path_and_content_discoverable_required:true,windows_timeout_term_ctrl_break_forbidden:true,windows_timeout_graceful_then_forced_tree_termination_required:true,windows_timeout_300ms_convergence_test_max_ms:1800,cmd_rmdir_full_ordinary_cleanup_not_privileged_by_surface_syntax:true,schema38_contract_ratified:true,schema38_earliest_owner_pr:"LB-006",schema38_next_g2_review_generation:26,schema38_next_g3_review_generation:16}))if(r[k]!==v)fail.push(k);
for(const k of ["workspace_bound_public_path_inputs_relative_only","workspace_public_absolute_path_input_forbidden","agent_workflow_workspace_relative_project_path_selector_required"])if(Object.hasOwn(r,k))fail.push("legacy:"+k);
const core=["workspace_context","agent_workflow","exec_command","command_control","task_control","git_workflow","document_workflow","view_image"];
if(JSON.stringify(r.localbridge_agent_api_v1_core_tools)!==JSON.stringify(core))fail.push("core-tools");
for(const e of ["PolicyDenied","WorkspaceDenied","RuntimeUnavailable","InvalidShellSyntax","PrivilegedRouteUnavailable","ProcessTimedOut"])if(!(r.stable_public_error_codes||[]).includes(e))fail.push("error:"+e);
const facade=readFileSync(resolve(root,"src-tauri/src/mcp/facade.rs"),"utf8");
for(const m of ["permission_mode","workspace_scope","ordinary_route_token","elevated_route_available","shell_discovery","capabilities","PrivilegedRouteUnavailable","InvalidShellSyntax"])if(!facade.includes(m))fail.push("workspace-context:"+m);
const policy=readFileSync(resolve(root,"src-tauri/src/mcp/policy.rs"),"utf8");
for(const m of ["powershell_readonly_command_discovery","cmd_invocation_requires_review","full_style_diagnostics_are_not_privileged_by_argument_tokens"])if(!policy.includes(m))fail.push("shell-classifier:"+m);
const state=JSON.parse(readFileSync(resolve(root,"PR_INDEX.json"),"utf8"));
const pr=(id)=>state.prs.find((item)=>item.id===id);
const g3Reopened=state.execution?.current_group==="G3" || ["READY","IN_PROGRESS","REWORK_REQUIRED","PASS"].includes(pr("LB-015")?.status);
if(g3Reopened){
  const app=readFileSync(resolve(root,"src/App.tsx"),"utf8");
  if(!app.includes(">权限模式</span>")||!app.includes("accessText[projection.permission]"))fail.push("dashboard-permission-row");
  if(app.includes(">管理员权限</span>"))fail.push("dashboard-privilege-row");
}
const lb016Active=["READY","IN_PROGRESS","REWORK_REQUIRED","PASS"].includes(pr("LB-016")?.status);
if(lb016Active){
  const ui=readFileSync(resolve(root,"src-tauri/src/commands/ui.rs"),"utf8");
  if(!/(challenge|not_before|not-before)/i.test(ui))fail.push("backend-admin-consent-challenge");
}
if(fail.length){console.error("ARCH-034 "+fail.join("|"));process.exit(1);}
console.log("ARCH-034 PASS");
