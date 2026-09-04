use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::execution::shell::ShellSelector;

use crate::control_plane::command_control::COMMAND_CONTROL_TRANSPORT_HEADROOM_MS;

pub const AGENT_API_VERSION: u32 = 1;
pub const AGENT_API_REVISION: u32 = 50;
pub const V1_CORE_TOOL_NAMES: [&str; 9] = [
    "workspace_context",
    "agent_workflow",
    "filesystem",
    "exec_command",
    "command_control",
    "task_control",
    "git_workflow",
    "document_workflow",
    "view_image",
];
pub(crate) const COMMAND_CONTROL_ACTIONS: [&str; 5] = ["adopt", "poll", "read", "write", "kill"];
pub(crate) const TASK_CONTROL_ACTIONS: [&str; 3] = ["list", "get", "cancel"];
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolRegistry;

impl ToolRegistry {
    pub const fn version(&self) -> u32 {
        AGENT_API_VERSION
    }

    pub fn contains(&self, name: &str) -> bool {
        V1_CORE_TOOL_NAMES.contains(&name)
    }

    pub fn core_tools(&self) -> Vec<Value> {
        V1_CORE_TOOL_NAMES
            .iter()
            .map(|name| public_tool_schema(name))
            .collect()
    }
}

pub(crate) fn public_tool_schema(name: &str) -> Value {
    let (description, input_schema) = match name {
        "workspace_context" => (
            "Return stable LocalBridge workspace/runtime context.",
            json!({
                "type":"object",
                "properties":{
                    "detail":{"type":"string","enum":["compact","full"],"default":"compact"}
                },
                "additionalProperties":false
            }),
        ),
        "agent_workflow" => (
            "Run a LocalBridge engineering workflow using stable actions. Resume after MCP reconnect requires both the returned task_id and adoption_token; an execution with an active owner cannot be transferred. Other actions may use objective/path/directory_changes/patch/commands as permitted by policy.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["diagnose","bugfix","feature","refactor","test_failure","build_release","document","resume","custom"],"description":"resume never starts a new workflow; all other actions start or inspect a workflow."},
                    "phase":{"type":"string","enum":["prepare","edit","verify","persist"],"description":"Optional coding-agent-v1 phase. Omit for legacy one-shot behavior."},
                    "task_id":{"type":"string","minLength":1,"description":"Required for edit/verify/persist phased calls and for resume after MCP reconnect; returned by prepare."},
                    "adoption_token":{"type":"string","minLength":1,"description":"Required with task_id when a durable workflow is resumed from a different MCP session."},
                    "objective":{"type":"string","description":"Optional for non-resume actions; forbidden for resume."},
                    "path":{"type":"string","default":".","description":"Optional project path for non-resume actions; forbidden for resume."},
                    "patch":{"type":"string","minLength":1,"description":"Optional single-file workspace patch for write-capable non-resume actions; forbidden for resume. Multi-file transactions and embedded moves are not supported."},
                    "expected_files":{"type":"object","additionalProperties":{"type":"string","minLength":64,"maxLength":64},"description":"Content SHA-256 identities required before phased edits."},
                    "directory_changes":{
                        "type":"array","maxItems":32,
                        "items":{
                            "type":"object",
                            "properties":{
                                "action":{"type":"string","enum":["create_directory","remove_empty_directory"]},
                                "path":{"type":"string","minLength":1}
                            },
                            "required":["action","path"],
                            "additionalProperties":false
                        }
                    },
                    "commands":{"description":"Optional process steps for process-capable non-resume actions; forbidden for resume.",
                        "type":"array","maxItems":8,
                        "items":{
                            "type":"object",
                            "properties":{
                                "command":{"type":"string","minLength":1},
                                "shell":{"type":"string","enum":["auto","powershell","pwsh","windows_powershell","cmd"],"default":"auto"},
                                "workdir":{"type":"string","default":".","description":"Relative to agent_workflow.path selected project; dot means the selected project. Do not repeat the project path here."},
                                "timeout_ms":{"type":"integer","minimum":1,"maximum":600000,"default":30000},
                                "yield_time_ms":{"type":"integer","minimum":0,"maximum":30000,"default":10000},
                                "max_output_bytes":{"type":"integer","minimum":1,"maximum":1048576,"default":65536},
                                "stdin":{"type":"string"}
                            },
                            "required":["command"],
                            "additionalProperties":false
                        }
                    }
                },
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "filesystem" => (
            "Perform bounded LocalBridge-owned filesystem operations. Action-specific required fields are enforced by the server; search matches names and metadata, while search_content searches UTF-8 file contents; replace and patch are identity-bound edits.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["list","stat","read","write","replace","patch","search","search_content","copy","move","delete","hash"]},
                    "path":{"type":"string","minLength":1,"description":"Required for list/stat/read/write/replace/search/search_content/delete/hash."},
                    "source":{"type":"string","minLength":1,"description":"Required for copy/move."},
                    "destination":{"type":"string","minLength":1,"description":"Required for copy/move."},
                    "recursive":{"type":"boolean","default":false,"description":"List and search are non-recursive by default; set true only when recursive traversal is required."},
                    "max_depth":{"type":"integer","minimum":1,"maximum":64},
                    "max_entries":{"type":"integer","minimum":1,"maximum":100000},
                    "max_results":{"type":"integer","minimum":1,"maximum":10000},
                    "offset":{"type":"integer","minimum":0},
                    "max_bytes":{"type":"integer","minimum":1,"maximum":1048576},
                    "content":{"type":"string","description":"Required for write; interpreted according to encoding."},
                    "encoding":{"type":"string","enum":["utf8","base64"]},
                    "expected_sha256":{"type":"string","minLength":64,"maxLength":64,"description":"Required for replace; identifies the exact source bytes."},
                    "old":{"type":"string","minLength":1,"description":"Required for replace; must occur exactly once."},
                    "new":{"type":"string","description":"Required for replace."},
                    "patch":{"type":"string","minLength":1,"description":"Required for patch; uses the LocalBridge patch envelope and modifies exactly one file per request."},
                    "expected_files":{"type":"object","additionalProperties":{"type":"string","minLength":64,"maxLength":64},"description":"Optional path-to-SHA-256 preconditions for patch."},
                    "pattern":{"type":"string","minLength":1,"description":"Required for search and search_content; search matches names, search_content matches literal UTF-8 text."},
                    "case_sensitive":{"type":"boolean","default":true,"description":"Used by search_content only."},
                    "max_file_bytes":{"type":"integer","minimum":1,"maximum":16777216,"description":"Per-file read bound for search_content."},
                    "type":{"type":"string","enum":["file","directory"]},
                    "min_size":{"type":"integer","minimum":0},
                    "max_size":{"type":"integer","minimum":0},
                    "modified_after":{"type":"integer","minimum":0,"description":"Unix epoch milliseconds."},
                    "modified_before":{"type":"integer","minimum":0,"description":"Unix epoch milliseconds."},
                    "sort_by":{"type":"string","enum":["path","size","modified"]},
                    "sort_order":{"type":"string","enum":["asc","desc"]},
                    "overwrite":{"type":"boolean"},
                    "calculate_size":{"type":"boolean"}
                },
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "exec_command" => (
            "Execute a command through a trusted logical shell under the current Windows user token. Full shell execution includes the same current-user authority for the shell and every descendant; administrator-token work is available only through the structured Broker route.",
            exec_command_input_schema(),
        ),
        "command_control" => (
            "Read, write, poll, or terminate an owned LocalBridge command session. Adopt requires an orphaned execution and its one-time adoption_token; it cannot take over another active owner.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":COMMAND_CONTROL_ACTIONS,"description":"adopt/poll/write/kill use session_id; read uses output_ref."},
                    "session_id":{"type":"string","minLength":1,"description":"Required for adopt, poll, write, and kill."},
                    "adoption_token":{"type":"string","minLength":1,"description":"One-time credential returned by exec_command or the previous successful adopt; required for adopt."},
                    "output_ref":{"type":"string","minLength":1,"description":"Required for read."},
                    "chars":{"type":"string","minLength":1,"description":"Required for write."},
                    "signal":{"type":"string","enum":["TERM","KILL","INT"],"description":"Optional kill signal; defaults to TERM."},
                    "wait_ms":{"type":"integer","minimum":0,"maximum":30000,"description":format!("Server-side wait target for poll/write/kill. Under a responsive local runtime LocalBridge returns within wait_ms plus at most {COMMAND_CONTROL_TRANSPORT_HEADROOM_MS}ms transport headroom; the budget is end-to-end and is not reset per socket stage.")},
                    "stream":{"type":"string","enum":["stdout","stderr"],"description":"Optional read stream."},
                    "offset":{"type":"integer","minimum":0,"description":"Optional read byte offset."},
                    "limit":{"type":"integer","minimum":1,"maximum":1048576,"description":"Optional read byte limit."}
                },
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "task_control" => (
            "List, read, or cancel LocalBridge tasks and detached executions.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":TASK_CONTROL_ACTIONS},
                    "task_id":{"type":"string","minLength":1,"description":"Optional for get and cancel. Required when cancel has more than one candidate."}
                },
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "git_workflow" => (
            "Run a stable LocalBridge Git workflow action. blame requires path; status/diff/log/show default path to the active project. Action-specific optional fields are documented on each property.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["status","diff","log","show","blame"],"description":"Selects the strict server-side argument contract."},
                    "path":{"type":"string","description":"Required for blame; optional repository/project context for status/diff/log/show."},
                    "paths":{"type":"array","items":{"type":"string"},"description":"Optional path filters for diff/show only."},
                    "ref":{"type":"string","description":"Optional log starting ref only."},
                    "rev":{"type":"string","description":"Optional revision for show/blame only."},
                    "staged":{"type":"boolean","description":"Optional diff only."},
                    "unstaged":{"type":"boolean","description":"Optional diff only."},
                    "include_untracked":{"type":"boolean","description":"Optional status only."},
                    "include_patch":{"type":"boolean","description":"Optional show only; defaults true."},
                    "max_entries":{"type":"integer","minimum":1,"description":"Optional status limit only."},
                    "max_count":{"type":"integer","minimum":1,"description":"Optional log count only."},
                    "skip":{"type":"integer","minimum":0,"description":"Optional log offset only."},
                    "start_line":{"type":"integer","minimum":1,"description":"Optional blame start line only."},
                    "end_line":{"type":"integer","minimum":1,"description":"Optional blame end line only."},
                    "max_lines":{"type":"integer","minimum":1,"description":"Optional blame line limit only."},
                    "context_lines":{"type":"integer","minimum":0,"description":"Optional diff/show context only."},
                    "max_bytes":{"type":"integer","minimum":1,"description":"Optional diff/show capture limit only."}
                },
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "document_workflow" => (
            "Inspect, search, create, edit, convert, or rebuild TXT, Markdown, DOCX, and PDF workspace documents through one DocumentIR pipeline. edit and rebuild require expected_sha256; PDF is read-only and can be converted to TXT or Markdown.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["inspect","search","create","edit","convert","rebuild"],"description":"Selects the strict action-specific argument contract."},
                    "path":{"type":"string","description":"Target document path. Required for every action; for convert this is the new output path."},
                    "source":{"type":"string","description":"Required only for convert."},
                    "content":{"type":"string","description":"Required for create and rebuild. Interpreted using source_format."},
                    "source_format":{"type":"string","enum":["text","markdown"],"description":"Optional for create/rebuild; defaults to markdown for .md/.docx targets and text for .txt targets."},
                    "expected_sha256":{"type":"string","minLength":64,"maxLength":64,"description":"Required for edit and rebuild. Use the sha256 returned by inspect/search."},
                    "edits":{"type":"array","minItems":1,"description":"Required only for edit. Operations are applied to one DocumentIR and committed once.","items":{"type":"object","properties":{"operation":{"type":"string","enum":["replace","insert_before","insert_after","delete"]},"block_id":{"type":"string","pattern":"^block-[1-9][0-9]*$"},"content":{"type":"string","description":"Required except for delete; one DocumentIR block, so newline characters are rejected."}},"required":["operation","block_id"],"additionalProperties":false}},
                    "query":{"type":"string","minLength":1,"description":"Required only for search."},
                    "case_sensitive":{"type":"boolean","description":"Optional search matching mode; defaults false."},
                    "max_results":{"type":"integer","minimum":1,"maximum":1000,"description":"Optional search result limit; defaults 100."},
                    "start_block":{"type":"integer","minimum":1,"description":"Optional inspect starting DocumentIR block; defaults 1."},
                    "max_blocks":{"type":"integer","minimum":1,"maximum":10000,"description":"Optional inspect block limit; defaults 200."},
                    "max_bytes":{"type":"integer","minimum":1,"maximum":1048576,"description":"Optional inspect text budget; defaults 1 MiB."}
                },
                "required":["action"],
                "additionalProperties":false
            }),
        ),
        "view_image" => (
            "Inspect a workspace image through the stable LocalBridge image contract.",
            json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string","minLength":1},
                    "max_bytes":{"type":"integer","minimum":1024,"maximum":10485760},
                    "max_width":{"type":"integer","minimum":1,"maximum":10000},
                    "max_height":{"type":"integer","minimum":1,"maximum":10000},
                    "auto_resize":{"type":"boolean"}
                },
                "required":["path"],
                "additionalProperties":false
            }),
        ),
        _ => unreachable!("registry only requests frozen public tools"),
    };
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "outputSchema": public_tool_output_schema(name)
    })
}

pub(crate) fn stable_public_tool_catalog() -> Value {
    json!({"tools":ToolRegistry.core_tools()})
}

fn public_tool_output_schema(name: &str) -> Value {
    let data_schema = match name {
        "workspace_context" => json!({
            "type":"object",
            "properties":{
                "api_version":{"type":"integer"},
                "facade_revision":{"type":"integer"},
                "workspace":{"type":"string"},
                "default_cwd":{"type":"string"},
                "runtime":{"type":"string","enum":["ready","recovering","fault","unavailable"]},
                "runtime_health":{"type":"object","additionalProperties":true},
                "detail":{"type":"string","enum":["compact","full"]},
                "coding_profile":{"type":"string","enum":["coding-agent-v1"]},
                "coding_capabilities":{"type":"array","items":{"type":"string"}},
                "git_root":{"type":["string","null"]},
                "important_files":{"type":"array","items":{"type":"string"}},
                "instructions":{"type":"array","items":{"type":"string"}},
                "permission_mode":{"type":"string","enum":["edit","full","elevated"]},
                "workspace_scope":{"type":"string","enum":["structured_tools_active_workspace","administrator_broker_paths"]},
                "ordinary_route_token":{"type":"string","enum":["current_windows_user"]},
                "elevated_route_available":{"type":"boolean"},
                "privilege_state":{"type":"string"},
                "broker_state":{"type":"string"},
                "uac_state":{"type":"string"},
                "administrator_token_available":{"type":"boolean"},
                "selected_route":{"type":"string"},
                "authority":{
                    "type":"object",
                    "properties":{
                        "desired_permission":{"type":"string","enum":["edit","full","elevated"]},
                        "observed_privilege":{"type":"string"},
                        "observed_broker":{"type":"string"},
                        "observed_uac":{"type":"string"},
                        "effective_permission":{"type":"string","enum":["edit","full","elevated"]},
                        "reconciliation":{"type":"string","enum":["converged","authorization_required","awaiting_authorization","broker_unavailable","disable_pending","unavailable"]},
                        "revision":{"type":"integer","minimum":0}
                    },
                    "required":["desired_permission","observed_privilege","observed_broker","observed_uac","effective_permission","reconciliation","revision"],
                    "additionalProperties":false
                },
                "shell_discovery":{"type":"object","additionalProperties":true},
                "capabilities":{"type":"object","additionalProperties":true},
                "project_name":{"type":["string","null"]},
                "project_type":{"type":["string","null"]},
                "project_version":{"type":["string","null"]},
                "git_branch":{"type":["string","null"]},
                "git_dirty":{"type":["boolean","null"]},
                "git_changed_count":{"type":["integer","null"],"minimum":0},
                "package_manager":{"type":["string","null"]},
                "build_system":{"type":["string","null"]},
                "test_system":{"type":["string","null"]},
                "runtime_availability":{"type":"object","additionalProperties":true},
                "trusted_shells":{"type":"array","items":{"type":"string"}},
                "current_task":{"type":["object","null"],"additionalProperties":true}
            },
            "required":["api_version","facade_revision","workspace","default_cwd","runtime","permission_mode","workspace_scope","ordinary_route_token","elevated_route_available","privilege_state","authority","shell_discovery","capabilities","project_name","project_type","project_version","git_branch","git_dirty","git_changed_count","package_manager","build_system","test_system","runtime_availability","trusted_shells","current_task"],
            "additionalProperties":false
        }),
        "agent_workflow" => json!({
            "type":"object",
            "properties":{
                "action":{"type":"string","enum":["diagnose","bugfix","feature","refactor","test_failure","build_release","document","resume","custom"]},
                "phase":{"type":["string","null"]},
                "workflow_id":{"type":["string","null"]},
                "task_id":{"type":["string","null"]},
                "adoption_token":{"type":["string","null"]},
                "objective":{"type":["string","null"]},
                "state":{"type":"string","enum":["context_ready","prepared","editing","verifying","persisted","running","completed","cancelled","failed"]},
                "summary":{"type":["string","null"]},
                "warnings":{"type":"array","items":{"type":"string"}},
                "next_step":{"type":["string","null"]},
                "output_refs":{"type":"array","items":{"type":"string"}},
                "workspace":{"type":"object","additionalProperties":true},
                "project":{"type":"object","additionalProperties":true},
                "context":{"type":"object","additionalProperties":true},
                "verification_plan":{"type":"array","items":{"type":"object","additionalProperties":true}},
                "git_before":{"type":["object","null"],"additionalProperties":true},
                "git_after":{"type":["object","null"],"additionalProperties":true},
                "completed":{"type":"boolean"},
                "modified_files":{"type":"array","items":{"type":"string"}},
                "test_results":{"type":"array","items":{"type":"object","additionalProperties":true}},
                "build_results":{"type":"array","items":{"type":"object","additionalProperties":true}},
                "failure":{"type":["object","null"],"additionalProperties":true},
                "patch_applied":{"type":"boolean"},
                "directory_changes":{"type":"array","items":{"type":"object","additionalProperties":true}},
                "commands":{"type":"array","items":{"type":"object","additionalProperties":true}},
                "current_execution":{"type":"object","additionalProperties":true}
            },
            "required":["action","state"],
            "additionalProperties":false
        }),
        "filesystem" => json!({
            "type":"object",
            "properties":{
                "entries":{"type":"array","items":{"type":"object","additionalProperties":true}},
                "matches":{"type":"array","items":{"type":"object","additionalProperties":false,"properties":{"path":{"type":"string"},"line":{"type":"integer","minimum":1},"column":{"type":"integer","minimum":1},"text":{"type":"string"}},"required":["path","line","column","text"]}},
                "affected_files":{"type":"array","items":{"type":"string"}},
                "scanned_entries":{"type":"integer","minimum":0},
                "scanned_files":{"type":"integer","minimum":0},
                "skipped_binary_files":{"type":"integer","minimum":0},
                "skipped_oversized_files":{"type":"integer","minimum":0},
                "truncated":{"type":"boolean"},
                "path":{"type":"string"},
                "kind":{"type":"string"},
                "size":{"type":"integer","minimum":0},
                "modified_ms":{"type":["integer","null"],"minimum":0},
                "calculated_size":{"type":"boolean"},
                "offset":{"type":"integer","minimum":0},
                "total_bytes":{"type":"integer","minimum":0},
                "returned_bytes":{"type":"integer","minimum":0},
                "eof":{"type":"boolean"},
                "encoding":{"type":"string","enum":["utf8","base64"]},
                "content":{"type":"string"},
                "destination":{"type":["string","null"]},
                "bytes":{"type":"integer","minimum":0},
                "changed":{"type":"boolean"},
                "algorithm":{"type":"string","enum":["sha256"]},
                "sha256":{"type":"string","minLength":64,"maxLength":64}
            },
            "additionalProperties":false
        }),
        "exec_command" => command_output_data_schema(),
        "command_control" => json!({
            "type":"object",
            "properties":{
                "status":{"type":"string","enum":["running","completed","failed","timed_out","cancelled","lost"]},
                "task_id":{"type":"string"},
                "execution_id":{"type":"string"},
                "adoption_token":{"type":"string"},
                "adopted":{"type":"boolean"},
                "elapsed_ms":{"type":"integer","minimum":0},
                "exit_code":{"type":"integer"},
                "signal":{"type":"string"},
                "session_id":{"type":"string"},
                "output":{"type":"string"},
                "output_ref":{"type":"string"},
                "output_refs":{"type":"object","additionalProperties":{"type":"string"}},
                "truncated":{"type":"boolean"},
                "stream":{"type":"string","enum":["stdout","stderr"]},
                "offset":{"type":"integer"},
                "requested_offset":{"type":"integer"},
                "limit":{"type":"integer"},
                "next_offset":{"type":["integer","null"]},
                "total_bytes":{"type":"integer","minimum":0},
                "returned_bytes":{"type":"integer","minimum":0},
                "content":{"type":"string"}
            },
            "additionalProperties":false
        }),
        "task_control" => json!({
            "type":"object",
            "properties":{
                "state":{"type":"string","enum":["idle","active","waiting","cancel_requested"]},
                "availability":{"type":"string","enum":["ready","stale","unknown","unavailable"]},
                "execution_state":{"type":"string"},
                "kind":{"type":["string","null"]},
                "summary":{"type":["string","null"]},
                "task_id":{"type":["string","null"]},
                "session_id":{"type":["string","null"]},
                "current_activity":{"type":["object","null"],"additionalProperties":true},
                "last_activity":{"type":["object","null"],"additionalProperties":true},
                "scheduler":{"type":"object","additionalProperties":true},
                "cancelled_requests":{"type":"integer","minimum":0},
                "cancellation_requested":{"type":"boolean"},
                "cancelled_queued_tasks":{"type":"integer","minimum":0},
                "durable_task_cancelled":{"type":"boolean"},
                "workflow_cancelled":{"type":"boolean"},
                "task":{"type":["object","null"],"additionalProperties":true},
                "tasks":{"type":"array","items":{"type":"object","additionalProperties":true}},
                "executions":{"type":"array","items":{"type":"object","additionalProperties":true}}
            },
            "required":["state","availability"],
            "additionalProperties":false
        }),
        "git_workflow" => json!({
            "type":"object",
            "properties":{
                "is_repo":{"type":"boolean"},
                "repository_root":{"type":["string","null"]},
                "head":{"type":["string","null"]},
                "diff":{"type":"string"},
                "content":{"type":"string"},
                "entries":{"type":"array"},
                "files":{"type":"array"},
                "commits":{"type":"array"},
                "lines":{"type":"array"},
                "warnings":{"type":"array"}
            },
            "additionalProperties":true
        }),
        "document_workflow" => json!({
            "type":"object",
            "properties":{
                "action":{"type":"string","enum":["inspect","search","create","edit","convert","rebuild"]},
                "path":{"type":"string"},
                "source":{"type":"string"},
                "format":{"type":"string","enum":["text","markdown","docx","pdf"]},
                "source_format":{"type":"string","enum":["text","markdown","docx","pdf"]},
                "sha256":{"type":"string","minLength":64,"maxLength":64},
                "source_sha256":{"type":"string","minLength":64,"maxLength":64},
                "text":{"type":"string"},
                "blocks":{"type":"array","items":{"type":"object","properties":{"id":{"type":"string"},"kind":{"type":"string","enum":["paragraph","heading","list_item","blank"]},"text":{"type":"string"},"level":{"type":"integer","minimum":1,"maximum":6}},"required":["id","kind","text"],"additionalProperties":false}},
                "matches":{"type":"array","items":{"type":"object","properties":{"block_id":{"type":"string"},"block_index":{"type":"integer","minimum":1},"excerpt":{"type":"string"}},"required":["block_id","block_index","excerpt"],"additionalProperties":false}},
                "start_block":{"type":"integer","minimum":1},
                "end_block":{"type":["integer","null"],"minimum":1},
                "total_blocks":{"type":"integer","minimum":0},
                "total_bytes":{"type":"integer"},
                "bytes":{"type":"integer","minimum":0},
                "truncated":{"type":"boolean"},
                "applied_edits":{"type":"integer","minimum":1}
            },
            "additionalProperties":false
        }),
        "view_image" => json!({
            "type":"object",
            "properties":{
                "kind":{"const":"image"},
                "path":{"type":"string"},
                "mime_type":{"type":"string"},
                "original_width":{"type":"integer","minimum":1},
                "original_height":{"type":"integer","minimum":1},
                "width":{"type":"integer","minimum":1},
                "height":{"type":"integer","minimum":1},
                "resized":{"type":"boolean"}
            },
            "required":["kind","path","mime_type","original_width","original_height","width","height","resized"],
            "additionalProperties":false
        }),
        _ => unreachable!("registry only requests frozen public tools"),
    };
    json!({
        "type":"object",
        "properties":{
            "ok":{"type":"boolean"},
            "state":{"type":["string","null"]},
            "summary":{"type":["string","null"]},
            "task_id":{"type":["string","null"]},
            "warnings":{"type":"array","items":{"type":"string"}},
            "next_step":{"type":["string","null"]},
            "output_refs":{"type":"array","items":{"type":"string"}},
            "data":{"anyOf":[data_schema,{"type":"null"}]},
            "error":{"anyOf":[public_error_output_schema(),{"type":"null"}]}
        },
        "required":["ok","state","summary","task_id","warnings","next_step","output_refs","data","error"],
        "additionalProperties":false
    })
}

fn command_output_data_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "status":{"type":"string","enum":["running","completed","failed","cancelled","timed_out","lost"]},
            "task_id":{"type":"string"},
            "execution_id":{"type":"string"},
            "elapsed_ms":{"type":"integer","minimum":0},
            "exit_code":{"type":"integer"},
            "signal":{"type":"string"},
            "session_id":{"type":"string"},
            "adoption_token":{"type":"string"},
            "output":{"type":"string"},
            "output_ref":{"type":"string"},
            "output_refs":{"type":"object","additionalProperties":{"type":"string"}},
            "truncated":{"type":"boolean"},
            "allowed":{"type":"boolean"},
            "route":{"type":"string","enum":["ordinary","workspace_restricted","elevated_required","permanently_denied"]},
            "rule_category":{"type":"string"},
            "remediation":{"type":"string"},
            "would_execute":{"type":"boolean"}
        },
        "required":["status"],
        "additionalProperties":false
    })
}

pub(crate) fn public_error_output_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "code":{"type":"string","enum":[
                "InvalidArgument","NotFound","WorkspaceDenied","CapabilityDenied","PolicyDenied",
                "InvalidShellSyntax","ElevatedOperationNotReviewed","PrivilegedRouteUnavailable","ElevationRequired","ProcessFailed","ProcessTimedOut",
                "ProcessCancelled","OperationTimedOut","QueueCapacityExceeded","TaskIdRequired","TaskNotOwned","SessionUnavailable","OutputNotFound","OutputTruncated","RuntimeUnavailable","CapabilityUnavailable",
                "RuntimeProtocolMismatch","RuntimeCapabilityMismatch","FileChanged","PatchConflict","AmbiguousMatch","Internal"
            ]},
            "error_code":{"type":"string","enum":["InvalidRequest","Unavailable","Denied","Timeout","Cancelled","ExecutionFailed","Unknown"]},
            "phase":{"type":"string","enum":["transport","mcp","runtime","policy","tool","process","unknown"]},
            "cause":{"type":"string"},
            "http_status":{"type":["integer","null"],"minimum":100,"maximum":599},
            "message":{"type":"string"},
            "retryable":{"type":"boolean"},
            "rule_category":{"type":"string"},
            "remediation":{"type":"string"},
            "details":{"anyOf":[{"type":"object","additionalProperties":true},{"type":"null"}]}
        },
        "required":["code","error_code","phase","cause","message","retryable"],
        "additionalProperties":false
    })
}

#[cfg(test)]
pub(crate) const EXEC_COMMAND_FIELDS: [&str; 9] = [
    "command",
    "shell",
    "workdir",
    "timeout_ms",
    "yield_time_ms",
    "max_output_bytes",
    "stdin",
    "dry_run",
    "verbosity",
];

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExecVerbosity {
    Summary,
    Full,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecCommandWire {
    command: String,
    #[serde(default = "default_shell")]
    shell: ShellSelector,
    #[serde(default = "default_workdir")]
    workdir: String,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
    #[serde(default = "default_yield_time_ms")]
    yield_time_ms: u64,
    #[serde(default = "default_max_output_bytes")]
    max_output_bytes: u64,
    #[serde(default, deserialize_with = "present_string")]
    stdin: Option<String>,
    #[serde(default)]
    dry_run: bool,
    #[serde(default = "default_verbosity")]
    verbosity: ExecVerbosity,
}

#[derive(Debug)]
pub(crate) struct ExecCommandArguments {
    pub command: String,
    pub shell: ShellSelector,
    pub workdir: PathBuf,
    pub timeout_ms: u64,
    pub yield_time_ms: u64,
    pub max_output_bytes: usize,
    pub stdin: Option<String>,
    pub dry_run: bool,
}

impl ExecCommandArguments {
    pub(crate) fn parse(value: Value) -> Result<Self, ()> {
        let wire: ExecCommandWire = serde_json::from_value(value).map_err(|_| ())?;
        if wire.command.is_empty()
            || !(1..=600_000).contains(&wire.timeout_ms)
            || wire.yield_time_ms > 30_000
            || !(1..=1_048_576).contains(&wire.max_output_bytes)
        {
            return Err(());
        }
        let _ = wire.verbosity;
        Ok(Self {
            command: wire.command,
            shell: wire.shell,
            workdir: PathBuf::from(wire.workdir),
            timeout_ms: wire.timeout_ms,
            yield_time_ms: wire.yield_time_ms,
            max_output_bytes: wire.max_output_bytes as usize,
            stdin: wire.stdin,
            dry_run: wire.dry_run,
        })
    }
}

pub(crate) fn exec_command_input_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "command":{"type":"string","minLength":1,"description":"Shell text executed with current Windows user authority; descendants inherit that authority."},
            "shell":{"type":"string","enum":["auto","powershell","pwsh","windows_powershell","cmd"],"default":"auto"},
            "workdir":{"type":"string","default":"."},
            "timeout_ms":{"type":"integer","minimum":1,"maximum":600000,"default":30000},
            "yield_time_ms":{"type":"integer","minimum":0,"maximum":30000,"default":10000},
            "max_output_bytes":{"type":"integer","minimum":1,"maximum":1048576,"default":65536},
            "stdin":{"type":"string","default":""},
            "dry_run":{"type":"boolean","default":false,"description":"When true, return the policy decision without starting a command or creating a command session."},
            "verbosity":{"type":"string","enum":["summary","full"],"default":"summary","description":"Accepted compatibility hint; public structured data remains stable."}
        },
        "required":["command"],
        "additionalProperties":false
    })
}

const fn default_shell() -> ShellSelector {
    ShellSelector::Auto
}

fn default_workdir() -> String {
    ".".to_string()
}

const fn default_timeout_ms() -> u64 {
    30_000
}

const fn default_yield_time_ms() -> u64 {
    10_000
}

const fn default_max_output_bytes() -> u64 {
    65_536
}

const fn default_verbosity() -> ExecVerbosity {
    ExecVerbosity::Summary
}

fn present_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_and_parser_share_the_complete_field_contract() {
        let schema = exec_command_input_schema();
        let properties = schema["properties"].as_object().expect("properties");
        let mut schema_fields = properties.keys().map(String::as_str).collect::<Vec<_>>();
        let mut parser_fields = EXEC_COMMAND_FIELDS.to_vec();
        schema_fields.sort_unstable();
        parser_fields.sort_unstable();
        assert_eq!(schema_fields, parser_fields);

        let parsed = ExecCommandArguments::parse(json!({
            "command":"echo ready",
            "dry_run":true,
            "verbosity":"full"
        }))
        .expect("valid contract");
        assert!(parsed.dry_run);
        assert_eq!(parsed.workdir, PathBuf::from("."));
        assert!(ExecCommandArguments::parse(json!({"command":"echo", "extra":true})).is_err());
        assert!(ExecCommandArguments::parse(json!({"command":"echo", "timeout_ms":0})).is_err());
        assert!(ExecCommandArguments::parse(json!({"command":"echo", "dry_run":"yes"})).is_err());
        assert!(ExecCommandArguments::parse(json!({"command":"echo", "stdin":null})).is_err());
    }
}
