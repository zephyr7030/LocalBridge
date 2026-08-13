import { createHash } from "node:crypto";

const EXACT_COMMIT = /^[0-9a-f]{40}$/;
const EXACT_SHA256 = /^[0-9a-f]{64}$/;
const REVIEW_GOVERNANCE_PATHS = new Set(["PR_INDEX.json", "PROJECT_STATE.json"]);
const HUMAN_REVIEW_STATUSES = new Set(["BLOCKED", "REQUIRED", "PASS", "FAIL"]);
const PREAUTH_AUDIT_STATUSES = new Set(["PENDING", "PASS", "FAIL"]);
const EVIDENCE_DISPOSITIONS = new Set(["accepted_after_review", "independently_verified", "rejected", "unresolved"]);

export const PRE_G4_GATE_AUTHORIZATION = Object.freeze({
  id: "GOV-AUTH-G4-HUMAN-001",
  scheme: "user_file_sha256_v1",
  evidencePath: "governance/G4_HUMAN_GATE_AUTHORIZATION.txt",
  evidenceCommit: "15963a0ca227feac41e2bf522c5321562567c6f7",
  evidenceCanonicalSha256: "ebdc20eac990c77b6cd30a388792340bc0c1c0797187ba6ffcfac7e41cb61e7a",
  scope: "single_immediate_governance_commit",
  authorizedPaths: [
    "AGENTS.md",
    "ARCHITECTURE_RULES.json",
    "FINAL_REVIEW.json",
    "PR_CONTRACTS.json",
    "PR_INDEX.json",
    "PROJECT_STATE.json",
    "docs/06_PR_GROUPS_AND_EXECUTION.md",
    "docs/08_FINAL_REVIEW.md",
    "scripts/verify-architecture/g4-human-gate.mjs",
    "scripts/verify-architecture/g4-human-gate.test.mjs",
    "scripts/verify-architecture/governance-authorization.mjs",
    "scripts/verify-architecture/index.mjs",
    "skills/adversarial-review/SKILL.md",
    "templates/ADVERSARIAL_REVIEW_PROMPT.md",
    "templates/PR_EXECUTION_PROMPT.md",
  ],
});

export const G3_SIX_SCREEN_CONTRACT_RATIFICATION = Object.freeze({
  commit: "6084c69a03892a79ce066b2c532378dd2b2d3e44",
  paths: [
    "AGENTS.md",
    "PROJECT_STATE.json",
    "PR_CONTRACTS.json",
    "PR_INDEX.json",
    "START_HERE.md",
    "docs/01_PRODUCT_UX.md",
    "docs/04_UX_SPEC.md",
    "docs/06_PR_PLAN.md",
    "docs/07_ACCEPTANCE.md",
    "docs/07_ACCEPTANCE_MATRIX.md",
    "docs/08_FINAL_REVIEW.md",
    "skills/ui/SKILL.md",
  ],
});

const canonicalText = (value) => String(value ?? "").replace(/\r\n/g, "\n");
const canonicalSha256 = (value) => createHash("sha256").update(canonicalText(value), "utf8").digest("hex");
const normalizePaths = (values) => [...new Set(values ?? [])].map((value) => value.replaceAll("\\", "/")).sort();
const nonEmptyString = (value) => typeof value === "string" && value.trim().length > 0;

const LB016_AUTHORIZED_G3_REWORK_2026_08_13 = Object.freeze({
  oldSuccessTest: "screen 6 success message is 设置完成，尝试在 ChatGPT 中选择刚刚添加的连接器吧！",
  newSuccessTest: "screen 6 success message is 配置完成，在插件中选择刚刚添加的Local Bridge试试吧",
  addedArtifacts: [
    "screen 4 create-custom-plugin persisted-information panel",
    "fixed 900x620 non-resizable non-maximizable main window",
    "single edge-to-edge custom window chrome with native decorations disabled",
    "full-page onboarding layout using the fixed custom-chrome content area",
  ],
  addedTests: [
    "screen 4 title is 创建自定义插件",
    "screen 4 ChatGPT plugin-settings action opens only https://chatgpt.com/plugins#settings/Plugins in the system default browser through a fixed Rust allowlist",
    "screen 4 shows exactly two concise information rows 名称 Tunnel ID and does not show 本地服务",
    "screen 4 名称 value is Local Bridge",
    "screen 4 Tunnel ID value reflects the current persisted saved value rather than an unsaved frontend-only value",
    "each of the two screen 4 information rows has its own copy action and successful copy shows green 已复制 for exactly 3 seconds before restoring without layout shift",
    "screen 3 permission mode buttons preserve clearly visible balanced content-to-border spacing in the real 900x620 render and grow safely for wrapped descriptive text; final visual PASS requires human inspection and cannot be inferred from CSS padding markers alone",
    "the selected project runtime MCP and OpenAI Tunnel are all ready before screen 4 becomes reachable so plugin creation is executable rather than premature guidance",
    "onboarding never defers its only runtime startup edge until screen 5 or screen 6",
    "screens 4 and 5 use Local Bridge as the user-facing connector term",
    "main window is fixed to 900x620 with minimum and maximum 900x620 resizable false and maximizable false",
    "native window decorations are disabled and exactly one edge-to-edge custom chrome provides drag minimize and close without maximize or double frame",
    "onboarding uses the full fixed client content area without a centered floating card modal shell or large empty surrounding canvas",
  ],
  replacedTests: [
    ["screen 4 developer-mode guidance is 在插件设置页面最底端，打开“开发者模式”", "screen 4 custom connector guidance is concise and foolproof"],
    ["screen 4 lower plugin action label is 打开插件管理页", "screen 4 custom connector setup button"],
    ["screen 4 plugin management action opens only https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins in the system default browser through a fixed Rust allowlist", "screen 4 opens only https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins in the system default browser"],
  ],
});

const G3_HUMAN_REVIEW_AMENDMENT_2026_08_13 = Object.freeze({
  schemaVersion: 19,
  lb015AddedArtifacts: [
    "blue #0071e3 standard product accent with amber administrator-mode exception",
    "shared typed service-status dot presentation for Dashboard and onboarding",
  ],
  lb015AddedTests: [
    "primary and ordinary selected controls use the blue #0071e3 accent rather than black",
    "administrator mode uses amber logical selection styling and is not overridden by ordinary blue selected styling",
    "Dashboard tunnel and coding service states render status dots using Ready green Starting amber Fault red Unknown gray semantics from the same typed status source used by onboarding",
    "Dashboard does not maintain an independent conflicting service-status color state",
  ],
  lb016ArtifactReplacement: ["five-screen onboarding flow", "six-screen onboarding flow"],
  lb016AddedTests: [
    "screen 4 ChatGPT plugin-settings action is placed in the left-side action flow",
    "screen 4 shows 打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件 beneath the plugin-settings action",
    "screen 4 lower plugin-management action is placed in the left-side action flow",
    "ordinary selected permission modes use the standard blue accent while administrator mode uses amber logical styling in onboarding and Dashboard",
    "screen 4 provides an explicit back action to screen 3 and a continue action to screen 5",
    "screens 2 3 4 and 5 each provide an explicit back path and save start or configuration failure never traps the user",
    "screen 5 status dots map Ready to green Starting to amber Fault to red and Unknown to gray using the shared typed service-status source",
    "screen 5 provides an explicit back action to screen 4",
    "primary and ordinary selected wizard controls use the blue #0071e3 accent rather than black",
  ],
  lb016ReplacedTests: [
    ["onboarding has exactly five screens", "onboarding has exactly six screens"],
    ["onboarding never defers its only runtime startup edge until screen 5", "onboarding never defers its only runtime startup edge until screen 5 or screen 6"],
    ["screen 5 contains only local runtime environment coding service and OpenAI Tunnel checks", "screen 6 contains only local runtime environment coding service and OpenAI Tunnel checks"],
    ["screen 5 confirm is disabled until all three checks are green", "screen 6 confirm is disabled until all three checks are green"],
    ["screen 5 success message is hidden until all three checks are green", "screen 6 success message is hidden until all three checks are green"],
    ["screen 5 success message is 配置完成，在插件中选择刚刚添加的Local Bridge试试吧", "screen 6 success message is 配置完成，在插件中选择刚刚添加的Local Bridge试试吧"],
    ["screen 5 does not auto-advance", "screen 6 does not auto-advance"],
    ["screen 5 confirm enters main UI after readiness", "screen 6 confirm enters main UI after readiness"],
    ["onboarding has no sixth screen", "onboarding has no seventh screen"],
    ["screen 4 uses Local Bridge as the user-facing connector term", "screens 4 and 5 use Local Bridge as the user-facing connector term"],
  ],
  lb016RemovedTests: [
    "screen 5 provides only the minimum connector confirmation/use guidance and does not pretend to detect ChatGPT state",
    "if a connector endpoint is displayed or copied it comes from a typed Rust projection backed by verified tunnel or control-plane metadata",
    "frontend never derives a connector endpoint from Tunnel ID or fabricates one",
  ],
  lb016LegacyInsertBefore: "copy-success feedback reserves layout space and causes no layout shift",
});

const G3_HUMAN_REVIEW_GENERATION_2_AMENDMENT_2026_08_13 = Object.freeze({
  schemaVersion: 20,
  baselineCommit: "a19297a77688aebe1f3d807f28da6b3fad1dcbcb",
  baselineSchemaVersion: 19,
  addedRules: {
    visible_admin_mode_selection_requests_uac: true,
    separate_enable_admin_button_forbidden: true,
    leaving_admin_mode_disables_broker: true,
    background_admin_preference_auto_uac_forbidden: true,
    foreground_configured_launch_auto_starts_runtime: true,
    foreground_runtime_start_must_not_block_ui: true,
    frontend_is_typed_projection_only: true,
    frontend_runtime_readiness_polling_state_machine_forbidden: true,
    blocking_backend_work_on_ui_thread_forbidden: true,
    ui_responsiveness_under_slow_backend_required: true,
    dashboard_idle_text: "等待命令",
    dashboard_idle_row_must_remain_visible: true,
    dashboard_frontend_synthesized_task_state_forbidden: true,
    settings_connection_field_labels: ["Tunnel ID", "Runtime API Key"],
    runtime_api_key_user_facing_label: "Runtime API Key",
    runtime_api_key_label_translation_forbidden: true,
    settings_test_connection_button_forbidden: true,
    settings_connection_partial_update_required: true,
    settings_save_controlled_reconnect_on_effective_connection_change: true,
    settings_sections: ["常规", "连接", "权限"],
    settings_general_controls: ["开机启动", "关闭窗口后继续运行"],
    settings_footer_actions: ["打开欢迎页", "完成"],
    close_window_continue_running_setting_required: true,
    diagnostics_sections: ["运行状态", "项目", "日志"],
    diagnostics_actions: ["打开日志", "导出诊断", "完成"],
    diagnostics_engineering_generation_details_forbidden: true,
    onboarding_screen_3_permission_min_height_px: 80,
    dashboard_native_windows_folder_picker_required: true,
    dashboard_permission_mode_row_forbidden: true,
    dashboard_permission_mode_controls_forbidden: true,
    dashboard_permission_mode_change_forbidden: true,
    dashboard_permission_mode_uac_trigger_forbidden: true,
    permission_mode_edit_surfaces: ["settings", "onboarding_screen_3"],
    permission_mode_post_onboarding_edit_surface: "settings_only",
    dashboard_admin_privilege_status_read_only: true,
    cloudflared_final_bundle_forbidden: true,
    cloudflare_managed_tunnel_runtime_forbidden: true,
    cloudflare_historical_compatibility_evidence_may_remain_non_executable: true,
  },
  prs: {
    "LB-013": {
      addedWritablePaths: ["src-tauri/src/settings/**", "schema/settings/**", "tests/migrations/**"],
      addedArtifacts: ["persisted close-window behavior preference", "non-blocking backend execution boundary for desktop lifecycle work"],
      addedTests: [
        "关闭窗口后继续运行=true hides the window and leaves runtime/tray unchanged",
        "关闭窗口后继续运行=false performs orderly managed runtime and privileged broker cleanup then exits",
        "close-window behavior preference is versioned persisted and migration-safe",
        "desktop lifecycle blocking work does not execute on the UI/WebView event thread",
      ],
    },
    "LB-014": {
      addedArtifacts: ["foreground configured launch automatic runtime start"],
      addedTests: [
        "configured foreground UI launch automatically starts selected project runtime MCP and OpenAI Tunnel without a second user start action",
        "foreground window remains responsive and projects Starting Ready or Fault while backend startup runs",
        "开机启动 controls Windows login launch registration only and does not gate runtime start after a manual foreground launch",
      ],
    },
    "LB-015": {
      artifactReplacements: [["Settings-only post-onboarding Edit/Full/Elevated minimal permission UI", "Edit/Full/Elevated minimal permission UI"]],
      addedArtifacts: [
        "settings page with exact 常规 连接 权限 groups",
        "independent Tunnel ID and Runtime API Key replace/edit flow",
        "native Windows folder picker for Dashboard add-project flow",
        "backend-only CurrentTask truth with 等待命令 idle projection",
        "projection-only responsive frontend boundary",
      ],
      addedTests: [
        "Settings has no separate 启用管理员权限 button; visible 管理员模式 selection or reselection in Settings requests UAC when privilege is not Active",
        "leaving 管理员模式 closes the privileged call gate and disables the broker",
        "Dashboard read-only administrator privilege status plus Settings and onboarding privilege runtime state are sourced from PrivilegeState rather than permission preference alone",
        "Dashboard renders no 权限模式 row and no 编辑模式 完整模式 管理员模式 selection controls",
        "Dashboard cannot change PermissionMode or request UAC through a permission-mode control",
        "Settings is the only post-onboarding permission-mode editing surface while onboarding screen 3 remains the first-run permission selection surface",
        "Dashboard administrator privilege status is read-only and sourced from PrivilegeState",
        "Dashboard no-task state is always visible as 等待命令 and never 空闲",
        "real production MCP and Broker execution transitions backend CurrentTaskStatus to the Dashboard and terminal state returns to 等待命令",
        "frontend does not synthesize task state or own runtime readiness/retry state machines",
        "Dashboard 选择其他文件夹 uses the native Windows folder picker and does not open a raw path-entry primary flow",
        "Settings has exactly 常规 连接 权限 sections with 开机启动 and 关闭窗口后继续运行 in 常规",
        "Settings connection labels are exact Tunnel ID and Runtime API Key; Runtime API Key is not translated to 运行密钥",
        "Settings fixed connection view shows persisted Tunnel ID summary and Runtime API Key only as 已保存 or 未保存 with separate 更换 actions",
        "clicking 更换 enters edit state without revealing or prefilling the saved Runtime API Key plaintext",
        "changing only Tunnel ID does not require or mutate Runtime API Key",
        "changing only Runtime API Key does not mutate Tunnel ID",
        "Settings save validates changed fields then securely writes and performs controlled reconnect only when effective connection configuration changed while active/connecting",
        "Settings has no 测试连接 button",
        "Settings footer exposes only 打开欢迎页 and 完成 for page-level actions",
        "intentionally slow backend lifecycle operation does not freeze frontend interaction or typed projection refresh",
      ],
      removedTests: ["only enable/disable admin controls", "dashboard idle state shows only current-task idle status"],
    },
    "LB-016": {
      testReplacements: [["ordinary selected permission modes use the standard blue accent while administrator mode uses amber logical styling in onboarding and Settings", "ordinary selected permission modes use the standard blue accent while administrator mode uses amber logical styling in onboarding and Dashboard"]],
      addedArtifacts: ["backend-owned onboarding start/readiness state machine", "administrator-mode selection UAC activation"],
      addedTests: [
        "screen 3 visible selection or reselection of 管理员模式 is the explicit user action that requests UAC when broker is not Active; no separate enable-admin button exists",
        "background preference restore remains non-UAC even though visible administrator-mode selection requests UAC",
        "screen 3 permission buttons have structural min-height at least 80px or an equivalent provable layout for title plus two-line description and balanced vertical whitespace; human 900x620 visual Gate remains required",
        "React does not own runtime start readiness polling loop; backend owns start/readiness state and frontend only observes typed projection",
        "backend start/readiness delay does not make the onboarding UI unresponsive",
      ],
      removedTests: ["UAC only from explicit user action"],
    },
    "LB-017": {
      addedArtifacts: ["minimal diagnostics runtime status project and recent redacted log view", "open-log action"],
      removedArtifacts: ["minimal diagnostics"],
      addedTests: [
        "diagnostics has exactly user-facing sections 运行状态 项目 日志",
        "运行状态 shows 本地运行环境 编码服务 OpenAI Tunnel 管理员权限 from typed backend state",
        "项目 shows the actual current project path rather than only selected/unselected",
        "日志 shows a bounded recent redacted user-facing event list and never leaks Runtime API Key Authorization nonce or raw sensitive payload",
        "diagnostics page actions are only 打开日志 导出诊断 完成",
        "normal diagnostics UI does not expose Broker generation reconnect generation attempt counters IPC nonce PID SID or other engineering internals",
        "diagnostic export remains redacted and may contain safe typed detail without changing the minimal on-screen view",
        "no refresh retry-connection or open-welcome action appears on the diagnostics page",
      ],
      removedTests: [
        "no secret export",
        "safe repair boundaries",
        "typed checks",
        "broker status without nonce/secret exposure",
        "diagnostics can report reconnect generation/attempt history without exposing it as dashboard feed",
        "reconnect diagnostics contain no secrets",
      ],
    },
    "LB-018": {
      addedArtifacts: ["Cloudflare/cloudflared-free final LocalBridge runtime bundle"],
      addedTests: [
        "final runtime bundle and installer contain no cloudflared.exe or cloudflared-manifest.json",
        "runtime-manifest and packaging inventory contain no Cloudflare/cloudflared runtime entry",
        "LocalBridge launch arguments environment and fallback paths do not activate cloudflared or Cloudflare managed tunnel",
        "historical upstream compatibility evidence may mention cloudflared but is not copied into executable final runtime bundle",
        "packaging gate fails closed if cloudflared is reintroduced",
      ],
    },
  },
});

const G3_MANUAL_SUPPLEMENT_2026_08_14 = Object.freeze({
  schemaVersion: 21,
  baselineCommit: "e66a3b9a0d5a844fa35ed6f63b70acaeae4e4110",
  baselineSchemaVersion: 20,
  replacedRules: {
    dashboard_current_task_status: ["single_current_or_last_timing_projection", "single_ephemeral_projection"],
  },
  removedBaselineRules: {
    permission_mode_post_onboarding_edit_surface: "settings_only",
  },
  addedRules: {
    dashboard_restart_auto_uac_forbidden: true,
    dashboard_restart_backend_owned_controlled_single_owner: true,
    dashboard_restart_service_accent: "amber",
    dashboard_service_controls: ["重启服务", "关闭服务"],
    dashboard_service_controls_shared_button_language_required: true,
    dashboard_stop_orderly_managed_shutdown_required: true,
    dashboard_stop_records_manual_stop: true,
    dashboard_stop_service_accent: "red",
    peer_action_button_alignment_tolerance_css_px: 1,
    peer_action_button_left_edge_alignment_required: true,
    permission_mode_post_onboarding_edit_surfaces: ["settings", "explicitly_reopened_onboarding_screen_3"],
    persistent_runtime_fault_not_treated_as_transient_notice: true,
    reopened_onboarding_screen_3_permission_edit_allowed: true,
    settings_replace_buttons_same_action_column_required: true,
    task_active_elapsed_required: true,
    task_execution_timing_backend_owned: true,
    task_idle_last_command_age_format: { seconds_under_60: "nS前", minutes_under_60: "n分钟前", hours_from_1: "大于1小时", days: "大于n天" },
    task_idle_last_command_age_required: true,
    task_last_metadata_only_allowed: true,
    task_short_execution_must_not_be_missed: true,
    ui_transient_operation_notice_examples: ["无法准备管理员权限", "权限模式未更新", "一次性保存/选择失败"],
    ui_transient_operation_notice_seconds: 3,
    workspace_display_normalization_must_not_authorize: true,
    workspace_internal_resolved_verbatim_path_allowed: true,
    workspace_user_facing_verbatim_prefix_forbidden: true,
  },
  lb015: {
    addedWritablePaths: [
      "src-tauri/src/state/**", "src-tauri/src/mcp/**", "src-tauri/src/app/**", "src-tauri/src/lib.rs",
      "tests/integration/policy/**", "tests/integration/background/**",
    ],
    artifactReplacements: [
      ["post-onboarding Edit/Full/Elevated UI in Settings and explicitly reopened onboarding Screen3, with Dashboard remaining read-only", "Settings-only post-onboarding Edit/Full/Elevated minimal permission UI"],
      ["backend-only CurrentTask truth with elapsed timing and retained last-command time metadata so short real executions cannot be missed", "backend-only CurrentTask truth with 等待命令 idle projection"],
    ],
    addedArtifacts: [
      "user-facing standard Windows workspace path projection separated from internal verbatim resolved authority path",
      "transient three-second one-shot operation feedback",
      "shared left-edge action-column alignment for peer buttons",
      "Dashboard amber 重启服务 and red 关闭服务 controls backed by real managed lifecycle",
    ],
    testReplacements: [
      ["Dashboard never edits PermissionMode; Settings and an explicitly reopened onboarding screen 3 are both permitted permission-mode editing surfaces", "Settings is the only post-onboarding permission-mode editing surface while onboarding screen 3 remains the first-run permission selection surface"],
      ["Dashboard no-task state is always visible as 等待命令 and never 空闲; after at least one real task it also shows backend-grounded last-command relative age", "Dashboard no-task state is always visible as 等待命令 and never 空闲"],
      ["real production MCP and Broker execution transitions backend CurrentTaskStatus to the Dashboard, including executions completing faster than the frontend refresh interval, and active projection includes elapsed duration", "real production MCP and Broker execution transitions backend CurrentTaskStatus to the Dashboard and terminal state returns to 等待命令"],
      ["Settings save validates changed fields then securely writes and performs controlled reconnect when effective connection configuration changed while active or in any Starting/connecting state; asynchronous startup captured with old configuration cannot continue", "Settings save validates changed fields then securely writes and performs controlled reconnect only when effective connection configuration changed while active/connecting"],
    ],
    addedTests: [
      "create modify delete and ordinary command tool calls each update backend current/last task timing truth rather than depending on frontend polling luck",
      "task age formats cover nS前, n分钟前, 大于1小时 and 大于n天 while retaining no task history/feed/list",
      "one-shot operation messages such as 无法准备管理员权限 auto-dismiss after 3 seconds and do not become permanent page content; persistent runtime faults remain typed state",
      "Settings Tunnel ID and Runtime API Key 更换 buttons share the same action-column left edge within 1 CSS px at 900x620 regardless of summary text width",
      "peer action buttons across Dashboard Settings Diagnostics and onboarding follow the same horizontal alignment system instead of arbitrary per-row offsets",
      "Dashboard displays 重启服务 with amber semantics and 关闭服务 with red semantics using the existing shared button geometry and aligned action layout",
      "重启服务 is a backend-owned controlled restart that cancels or joins Starting Ready Recovering or Fault lifecycle state before starting exactly one current persisted configuration and never auto-prompts UAC",
      "关闭服务 records explicit manual-stop semantics and orderly closes privileged gate Broker Tunnel PEP MCP without closing the Dashboard window",
      "Dashboard Diagnostics and project UI render D:\\project rather than \\\\?\\D:\\project when they denote the same workspace while internal filesystem identity/resolved path remains unchanged for authorization",
    ],
  },
});

const G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14 = Object.freeze({
  schemaVersion: 22,
  baselineCommit: "d06ff536425c459c6f18a6e29f56bdf9b68d852f",
  baselineSchemaVersion: 21,
  removedBaselineRules: {
    workspace_internal_resolved_verbatim_path_allowed: true,
  },
  addedRules: {
    workspace_verbatim_path_allowed_only_for_internal_identity_validation: true,
    workspace_tool_invocation_verbatim_prefix_forbidden: true,
    workspace_command_cwd_verbatim_prefix_forbidden: true,
    workspace_sidecar_current_dir_verbatim_prefix_forbidden: true,
    workspace_execution_path_must_use_ordinary_win32_form: true,
    workspace_execution_path_derived_from_validated_identity_required: true,
    workspace_execution_path_normalization_must_not_authorize: true,
  },
  lb015: {
    addedWritablePaths: [
      "src-tauri/src/workspace/**",
      "src-tauri/src/runtime/**",
      "tests/unit/workspace/**",
      "tests/integration/workspace_switch/**",
      "tests/integration/autostart/**",
      "tests/integration/mcp/**",
      "tests/integration/process/**",
    ],
    artifactReplacements: [[
      "workspace filesystem-identity validation path separated from ordinary Win32 presentation and execution paths used by UI MCP Broker sidecars process launch and command tools",
      "user-facing standard Windows workspace path projection separated from internal verbatim resolved authority path",
    ]],
    testReplacements: [[
      "verbatim resolved paths such as \\\\?\\D:\\project are confined to internal filesystem identity validation and comparison; Dashboard Diagnostics WorkspaceRef runtime MCP Broker sidecar process launch and tool invocation cwd workdir current_dir or path arguments use ordinary D:\\project form tied to the same validated identity without changing authorization scope",
      "Dashboard Diagnostics and project UI render D:\\project rather than \\\\?\\D:\\project when they denote the same workspace while internal filesystem identity/resolved path remains unchanged for authorization",
    ]],
    addedTests: [
      "when GetFinalPathNameByHandleW resolves the selected workspace to \\\\?\\D:\\project the coding-tools command execution boundary receives ordinary D:\\project as cwd or workdir and a representative common command succeeds",
      "no MCP Broker sidecar ManagedProcessSpec process launch or command tool invocation receives a workspace cwd workdir current_dir or path argument beginning with the Win32 verbatim prefix",
      "ordinary execution path is revalidated or identity-bound to the same freshly validated filesystem object and a mismatch fails closed rather than authorizing a path alias",
    ],
  },
});

const containsAll = (values, required) => Array.isArray(values) && (required ?? []).every((item) => values.includes(item));
const containsNone = (values, forbidden) => Array.isArray(values) && (forbidden ?? []).every((item) => !values.includes(item));
const removeItems = (values, removed) => (values ?? []).filter((item) => !(removed ?? []).includes(item));
const canonicalJson = (value) => JSON.stringify(value, (_key, candidate) => {
  if (!candidate || Array.isArray(candidate) || typeof candidate !== "object") return candidate;
  return Object.fromEntries(Object.keys(candidate).sort().map((key) => [key, candidate[key]]));
});

function insertAfter(values, anchor, item) {
  const result = [...values];
  if (result.includes(item)) return result;
  const index = result.indexOf(anchor);
  result.splice(index >= 0 ? index + 1 : result.length, 0, item);
  return result;
}

function hasExactG3HumanReviewGeneration2PrAmendment(prs) {
  for (const [id, delta] of Object.entries(G3_HUMAN_REVIEW_GENERATION_2_AMENDMENT_2026_08_13.prs)) {
    const pr = prs?.[id];
    if (!pr) return false;
    if (!containsAll(pr.writable_paths ?? [], delta.addedWritablePaths ?? [])) return false;
    if (!containsAll(pr.required_artifacts ?? [], delta.addedArtifacts ?? [])) return false;
    if (!containsNone(pr.required_artifacts ?? [], delta.removedArtifacts ?? [])) return false;
    for (const [current, old] of delta.artifactReplacements ?? []) {
      if (!pr.required_artifacts?.includes(current) || pr.required_artifacts?.includes(old)) return false;
    }
    if (!containsAll(pr.required_tests ?? [], delta.addedTests ?? [])) return false;
    if (!containsNone(pr.required_tests ?? [], delta.removedTests ?? [])) return false;
    for (const [current, old] of delta.testReplacements ?? []) {
      if (!pr.required_tests?.includes(current) || pr.required_tests?.includes(old)) return false;
    }
  }
  return true;
}

export function hasExactG3HumanReviewGeneration2Amendment(contractsDoc) {
  if (contractsDoc?.schema_version !== G3_HUMAN_REVIEW_GENERATION_2_AMENDMENT_2026_08_13.schemaVersion) return false;
  for (const [key, expected] of Object.entries(G3_HUMAN_REVIEW_GENERATION_2_AMENDMENT_2026_08_13.addedRules)) {
    if (JSON.stringify(contractsDoc?.rules?.[key]) !== JSON.stringify(expected)) return false;
  }
  return hasExactG3HumanReviewGeneration2PrAmendment(contractsDoc?.prs);
}

export function normalizeG3HumanReviewGeneration2Amendment(prs) {
  const normalized = structuredClone(prs ?? null);
  if (!normalized || !hasExactG3HumanReviewGeneration2PrAmendment(normalized)) return normalized;
  for (const [id, delta] of Object.entries(G3_HUMAN_REVIEW_GENERATION_2_AMENDMENT_2026_08_13.prs)) {
    const pr = normalized[id];
    if (!pr) continue;
    if (Array.isArray(pr.writable_paths)) pr.writable_paths = removeItems(pr.writable_paths, delta.addedWritablePaths);
    if (Array.isArray(pr.required_artifacts)) pr.required_artifacts = removeItems(pr.required_artifacts, delta.addedArtifacts);
    if (Array.isArray(pr.required_tests)) pr.required_tests = removeItems(pr.required_tests, delta.addedTests);
    if (Array.isArray(pr.required_artifacts)) {
      pr.required_artifacts = pr.required_artifacts.map((item) => {
        const replacement = (delta.artifactReplacements ?? []).find(([current]) => current === item);
        return replacement ? replacement[1] : item;
      });
    }
    if (Array.isArray(pr.required_tests)) {
      pr.required_tests = pr.required_tests.map((item) => {
        const replacement = (delta.testReplacements ?? []).find(([current]) => current === item);
        return replacement ? replacement[1] : item;
      });
    }
  }
  const lb015 = normalized["LB-015"];
  if (Array.isArray(lb015?.required_tests)) {
    lb015.required_tests = insertAfter(lb015.required_tests, "no admin time selector", "only enable/disable admin controls");
    lb015.required_tests = insertAfter(lb015.required_tests, "dashboard shows task kind summary and status for active task", "dashboard idle state shows only current-task idle status");
  }
  const lb016 = normalized["LB-016"];
  if (Array.isArray(lb016?.required_tests)) {
    lb016.required_tests = insertAfter(lb016.required_tests, "Elevated option present without TTL UI", "UAC only from explicit user action");
  }
  const lb017 = normalized["LB-017"];
  if (Array.isArray(lb017?.required_artifacts) && !lb017.required_artifacts.includes("minimal diagnostics")) {
    lb017.required_artifacts.unshift("minimal diagnostics");
  }
  if (Array.isArray(lb017?.required_tests)) {
    lb017.required_tests.unshift(
      "no secret export",
      "safe repair boundaries",
      "typed checks",
      "broker status without nonce/secret exposure",
      "diagnostics can report reconnect generation/attempt history without exposing it as dashboard feed",
      "reconnect diagnostics contain no secrets",
    );
  }
  return normalized;
}

function normalizeG3HumanReviewGeneration2Rules(rules) {
  const normalized = structuredClone(rules ?? null);
  if (!normalized) return normalized;
  for (const key of Object.keys(G3_HUMAN_REVIEW_GENERATION_2_AMENDMENT_2026_08_13.addedRules)) delete normalized[key];
  return normalized;
}

export function hasExactG3ManualSupplement20260814(contractsDoc) {
  if (contractsDoc?.schema_version !== G3_MANUAL_SUPPLEMENT_2026_08_14.schemaVersion) return false;
  const rules = contractsDoc?.rules;
  for (const [key, [current]] of Object.entries(G3_MANUAL_SUPPLEMENT_2026_08_14.replacedRules)) {
    if (JSON.stringify(rules?.[key]) !== JSON.stringify(current)) return false;
  }
  for (const key of Object.keys(G3_MANUAL_SUPPLEMENT_2026_08_14.removedBaselineRules)) {
    if (Object.hasOwn(rules ?? {}, key)) return false;
  }
  for (const [key, expected] of Object.entries(G3_MANUAL_SUPPLEMENT_2026_08_14.addedRules)) {
    if (JSON.stringify(rules?.[key]) !== JSON.stringify(expected)) return false;
  }
  const lb015 = contractsDoc?.prs?.["LB-015"];
  if (!lb015) return false;
  if (!containsAll(lb015.writable_paths, G3_MANUAL_SUPPLEMENT_2026_08_14.lb015.addedWritablePaths)) return false;
  if (!containsAll(lb015.required_artifacts, G3_MANUAL_SUPPLEMENT_2026_08_14.lb015.addedArtifacts)) return false;
  if (!containsAll(lb015.required_tests, G3_MANUAL_SUPPLEMENT_2026_08_14.lb015.addedTests)) return false;
  for (const [current, old] of G3_MANUAL_SUPPLEMENT_2026_08_14.lb015.artifactReplacements) {
    if (!lb015.required_artifacts?.includes(current) || lb015.required_artifacts?.includes(old)) return false;
  }
  for (const [current, old] of G3_MANUAL_SUPPLEMENT_2026_08_14.lb015.testReplacements) {
    if (!lb015.required_tests?.includes(current) || lb015.required_tests?.includes(old)) return false;
  }
  return true;
}

export function normalizeG3ManualSupplement20260814(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = G3_MANUAL_SUPPLEMENT_2026_08_14.baselineSchemaVersion;
  for (const [key, [, old]] of Object.entries(G3_MANUAL_SUPPLEMENT_2026_08_14.replacedRules)) normalized.rules[key] = old;
  for (const [key, old] of Object.entries(G3_MANUAL_SUPPLEMENT_2026_08_14.removedBaselineRules)) normalized.rules[key] = old;
  for (const key of Object.keys(G3_MANUAL_SUPPLEMENT_2026_08_14.addedRules)) delete normalized.rules[key];
  const lb015 = normalized.prs?.["LB-015"];
  if (lb015) {
    lb015.writable_paths = removeItems(lb015.writable_paths, G3_MANUAL_SUPPLEMENT_2026_08_14.lb015.addedWritablePaths);
    lb015.required_artifacts = removeItems(lb015.required_artifacts, G3_MANUAL_SUPPLEMENT_2026_08_14.lb015.addedArtifacts)
      .map((item) => G3_MANUAL_SUPPLEMENT_2026_08_14.lb015.artifactReplacements.find(([current]) => current === item)?.[1] ?? item);
    lb015.required_tests = removeItems(lb015.required_tests, G3_MANUAL_SUPPLEMENT_2026_08_14.lb015.addedTests)
      .map((item) => G3_MANUAL_SUPPLEMENT_2026_08_14.lb015.testReplacements.find(([current]) => current === item)?.[1] ?? item);
  }
  return normalized;
}

export function hasExactG3ManualPathExecutionCorrection20260814(contractsDoc) {
  if (contractsDoc?.schema_version !== G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.schemaVersion) return false;
  const rules = contractsDoc?.rules;
  for (const key of Object.keys(G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.removedBaselineRules)) {
    if (Object.hasOwn(rules ?? {}, key)) return false;
  }
  for (const [key, expected] of Object.entries(G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.addedRules)) {
    if (JSON.stringify(rules?.[key]) !== JSON.stringify(expected)) return false;
  }
  const lb015 = contractsDoc?.prs?.["LB-015"];
  if (!lb015) return false;
  if (!containsAll(lb015.writable_paths, G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.lb015.addedWritablePaths)) return false;
  if (!containsAll(lb015.required_tests, G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.lb015.addedTests)) return false;
  for (const [current, old] of G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.lb015.artifactReplacements) {
    if (!lb015.required_artifacts?.includes(current) || lb015.required_artifacts?.includes(old)) return false;
  }
  for (const [current, old] of G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.lb015.testReplacements) {
    if (!lb015.required_tests?.includes(current) || lb015.required_tests?.includes(old)) return false;
  }
  return true;
}

export function normalizeG3ManualPathExecutionCorrection20260814(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.baselineSchemaVersion;
  for (const [key, old] of Object.entries(G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.removedBaselineRules)) normalized.rules[key] = old;
  for (const key of Object.keys(G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.addedRules)) delete normalized.rules[key];
  const lb015 = normalized.prs?.["LB-015"];
  if (lb015) {
    lb015.writable_paths = removeItems(lb015.writable_paths, G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.lb015.addedWritablePaths);
    lb015.required_artifacts = lb015.required_artifacts
      .map((item) => G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.lb015.artifactReplacements.find(([current]) => current === item)?.[1] ?? item);
    lb015.required_tests = removeItems(lb015.required_tests, G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.lb015.addedTests)
      .map((item) => G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.lb015.testReplacements.find(([current]) => current === item)?.[1] ?? item);
  }
  return normalized;
}

export function hasExactG3HumanReviewAmendment(prs) {
  const lb015 = prs?.["LB-015"];
  const lb016 = prs?.["LB-016"];
  if (!Array.isArray(lb015?.required_artifacts)
    || !Array.isArray(lb015?.required_tests)
    || !Array.isArray(lb016?.required_artifacts)
    || !Array.isArray(lb016?.required_tests)) return false;
  const [currentArtifact, oldArtifact] = G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016ArtifactReplacement;
  return G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb015AddedArtifacts.every((item) => lb015.required_artifacts.includes(item))
    && G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb015AddedTests.every((item) => lb015.required_tests.includes(item))
    && lb016.required_artifacts.includes(currentArtifact)
    && !lb016.required_artifacts.includes(oldArtifact)
    && G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016AddedTests.every((item) => lb016.required_tests.includes(item))
    && G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016ReplacedTests.every(([current, old]) => lb016.required_tests.includes(current) && !lb016.required_tests.includes(old))
    && G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016RemovedTests.every((item) => !lb016.required_tests.includes(item));
}

export function normalizeG3HumanReviewAmendment(prs) {
  const normalized = structuredClone(prs ?? null);
  if (!hasExactG3HumanReviewAmendment(normalized)) return normalized;
  const lb015 = normalized["LB-015"];
  const lb016 = normalized["LB-016"];
  lb015.required_artifacts = lb015.required_artifacts.filter((item) => !G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb015AddedArtifacts.includes(item));
  lb015.required_tests = lb015.required_tests.filter((item) => !G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb015AddedTests.includes(item));
  const [currentArtifact, oldArtifact] = G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016ArtifactReplacement;
  lb016.required_artifacts = lb016.required_artifacts.map((item) => item === currentArtifact ? oldArtifact : item);
  lb016.required_tests = lb016.required_tests
    .filter((item) => !G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016AddedTests.includes(item))
    .map((item) => {
      const replacement = G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016ReplacedTests.find(([current]) => current === item);
      return replacement ? replacement[1] : item;
    });
  const insertAt = lb016.required_tests.indexOf(G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016LegacyInsertBefore);
  if (insertAt >= 0) lb016.required_tests.splice(insertAt, 0, ...G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016RemovedTests);
  return normalized;
}

function normalizeAuthorizedSemanticCorrections(prs) {
  const normalized = normalizeG3HumanReviewAmendment(normalizeG3HumanReviewGeneration2Amendment(prs));
  const lb016 = normalized?.["LB-016"];
  if (Array.isArray(lb016?.required_artifacts)) {
    lb016.required_artifacts = lb016.required_artifacts.filter((item) =>
      item !== "6-screen wizard"
      && !LB016_AUTHORIZED_G3_REWORK_2026_08_13.addedArtifacts.includes(item));
  }
  if (Array.isArray(lb016?.required_tests)) {
    lb016.required_tests = lb016.required_tests
      .filter((item) => !LB016_AUTHORIZED_G3_REWORK_2026_08_13.addedTests.includes(item))
      .map((item) => {
        if (item === LB016_AUTHORIZED_G3_REWORK_2026_08_13.newSuccessTest) return LB016_AUTHORIZED_G3_REWORK_2026_08_13.oldSuccessTest;
        const replacement = LB016_AUTHORIZED_G3_REWORK_2026_08_13.replacedTests.find(([current]) => current === item);
        return replacement ? replacement[1] : item;
      });
  }
  return normalized;
}

function requiredAuthorizationEvidenceFragments() {
  return [
    "G3 全部 PR 完成并通过独立对抗性智能体审查后，不得直接开启 G4",
    "人工实测细审核",
    "有权质疑、拒绝直接采信、要求复核或独立验证执行智能体",
    "用户和执行智能体的陈述均属于待验证证据",
    "authorization_id",
    "user_audit_status",
    "不得扩大任何普通 PR writable_paths",
    "唯一的紧随治理实现提交",
  ];
}

function validateLaterContractRatification(git, ratification) {
  const findings = [];
  if (!ratification || !EXACT_COMMIT.test(ratification.commit ?? "")
    || !git.commitExists(ratification.commit) || !git.isAncestor(ratification.commit)) {
    return ["later-contract-ratification-commit-missing"];
  }
  if (JSON.stringify(normalizePaths(git.commitPaths(ratification.commit)))
    !== JSON.stringify(normalizePaths(ratification.paths))) {
    findings.push("later-contract-ratification-commit-scope");
  }
  const ratified = git.jsonAt(ratification.commit, "PR_CONTRACTS.json");
  const lb015 = ratified?.prs?.["LB-015"];
  const lb016 = ratified?.prs?.["LB-016"];
  const rules = ratified?.rules;
  if (ratified?.schema_version !== 18
    || rules?.onboarding_screen_count !== 6
    || rules?.onboarding_screen_7_forbidden !== true
    || rules?.ui_button_visible_affordance_required !== true
    || rules?.ui_white_on_white_ambiguous_button_forbidden !== true
    || rules?.ui_minimum_prompt_required !== true
    || !lb015?.required_artifacts?.includes("coherent visible button token system shared across product UI")
    || !lb016?.required_artifacts?.includes("six-screen onboarding flow")
    || !lb016?.required_tests?.includes("onboarding has exactly six screens")) {
    findings.push("later-contract-ratification-content");
  }
  return findings;
}

export function validatePreG4GateAuthorization(
  contractsDoc,
  git,
  expected = PRE_G4_GATE_AUTHORIZATION,
  laterRatification = G3_SIX_SCREEN_CONTRACT_RATIFICATION,
) {
  const findings = [];
  let authorizationContracts = contractsDoc;
  if ((authorizationContracts?.schema_version ?? 0) >= G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.schemaVersion) {
    if (!hasExactG3ManualPathExecutionCorrection20260814(authorizationContracts)) {
      findings.push(`${expected.id}:manual-path-execution-correction-20260814-contract-amendment-drift`);
    }
    const correctionBaselineCommit = G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.baselineCommit;
    const correctionBaseline = git.commitExists(correctionBaselineCommit) && git.isAncestor(correctionBaselineCommit)
      ? git.jsonAt(correctionBaselineCommit, "PR_CONTRACTS.json")
      : null;
    const normalizedCorrection = normalizeG3ManualPathExecutionCorrection20260814(authorizationContracts);
    if (correctionBaseline?.schema_version !== G3_MANUAL_PATH_EXECUTION_CORRECTION_2026_08_14.baselineSchemaVersion) {
      findings.push(`${expected.id}:manual-path-execution-correction-20260814-baseline`);
    } else {
      if (JSON.stringify(normalizedCorrection?.prs) !== JSON.stringify(correctionBaseline.prs)) {
        findings.push(`${expected.id}:manual-path-execution-correction-20260814-pr-drift`);
      }
      if (canonicalJson(normalizedCorrection?.rules) !== canonicalJson(correctionBaseline.rules)) {
        findings.push(`${expected.id}:manual-path-execution-correction-20260814-rule-drift`);
      }
    }
    authorizationContracts = normalizedCorrection;
  }
  if ((authorizationContracts?.schema_version ?? 0) >= G3_MANUAL_SUPPLEMENT_2026_08_14.schemaVersion) {
    if (!hasExactG3ManualSupplement20260814(authorizationContracts)) {
      findings.push(`${expected.id}:manual-supplement-20260814-contract-amendment-drift`);
    }
    const supplementBaselineCommit = G3_MANUAL_SUPPLEMENT_2026_08_14.baselineCommit;
    const supplementBaseline = git.commitExists(supplementBaselineCommit) && git.isAncestor(supplementBaselineCommit)
      ? git.jsonAt(supplementBaselineCommit, "PR_CONTRACTS.json")
      : null;
    const normalizedSupplement = normalizeG3ManualSupplement20260814(authorizationContracts);
    if (supplementBaseline?.schema_version !== G3_MANUAL_SUPPLEMENT_2026_08_14.baselineSchemaVersion) {
      findings.push(`${expected.id}:manual-supplement-20260814-baseline`);
    } else {
      if (JSON.stringify(normalizedSupplement?.prs) !== JSON.stringify(supplementBaseline.prs)) {
        findings.push(`${expected.id}:manual-supplement-20260814-pr-drift`);
      }
      if (canonicalJson(normalizedSupplement?.rules) !== canonicalJson(supplementBaseline.rules)) {
        findings.push(`${expected.id}:manual-supplement-20260814-rule-drift`);
      }
    }
    authorizationContracts = normalizedSupplement;
  }
  if ((authorizationContracts?.schema_version ?? 0) >= G3_HUMAN_REVIEW_GENERATION_2_AMENDMENT_2026_08_13.schemaVersion) {
    if (!hasExactG3HumanReviewGeneration2Amendment(authorizationContracts)) {
      findings.push(`${expected.id}:human-review-generation2-contract-amendment-drift`);
    }
    const baselineCommit = G3_HUMAN_REVIEW_GENERATION_2_AMENDMENT_2026_08_13.baselineCommit;
    const baseline = git.commitExists(baselineCommit) && git.isAncestor(baselineCommit)
      ? git.jsonAt(baselineCommit, "PR_CONTRACTS.json")
      : null;
    if (baseline?.schema_version !== G3_HUMAN_REVIEW_GENERATION_2_AMENDMENT_2026_08_13.baselineSchemaVersion) {
      findings.push(`${expected.id}:human-review-generation2-baseline`);
    } else {
      if (JSON.stringify(normalizeG3HumanReviewGeneration2Amendment(authorizationContracts?.prs)) !== JSON.stringify(baseline.prs)) {
        findings.push(`${expected.id}:human-review-generation2-pr-drift`);
      }
      if (JSON.stringify(normalizeG3HumanReviewGeneration2Rules(authorizationContracts?.rules)) !== JSON.stringify(baseline.rules)) {
        findings.push(`${expected.id}:human-review-generation2-rule-drift`);
      }
    }
  }
  const schema19View = (authorizationContracts?.schema_version ?? 0) >= G3_HUMAN_REVIEW_GENERATION_2_AMENDMENT_2026_08_13.schemaVersion
    ? normalizeG3HumanReviewGeneration2Amendment(authorizationContracts?.prs)
    : authorizationContracts?.prs;
  if ((authorizationContracts?.schema_version ?? 0) >= G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.schemaVersion
    && !hasExactG3HumanReviewAmendment(schema19View)) {
    findings.push(`${expected.id}:human-review-contract-amendment-drift`);
  }
  const entries = authorizationContracts?.rules?.governance_authorizations;
  const entry = Array.isArray(entries) ? entries.find((candidate) => candidate?.id === expected.id) : null;
  if (!entry
    || entry.scheme !== expected.scheme
    || entry.evidence_path !== expected.evidencePath
    || entry.evidence_commit !== expected.evidenceCommit
    || entry.evidence_canonical_sha256 !== expected.evidenceCanonicalSha256
    || entry.scope !== expected.scope
    || entry.consumed !== true
    || entry.does_not_expand_pr_writable_paths !== true
    || JSON.stringify(normalizePaths(entry.authorized_paths)) !== JSON.stringify(normalizePaths(expected.authorizedPaths))) {
    return [`${expected.id}:contract-mismatch`];
  }
  if (!EXACT_COMMIT.test(expected.evidenceCommit) || !EXACT_SHA256.test(expected.evidenceCanonicalSha256)
    || !git.commitExists(expected.evidenceCommit) || !git.isAncestor(expected.evidenceCommit)) {
    findings.push(`${expected.id}:evidence-commit-missing`);
    return findings;
  }
  if (JSON.stringify(normalizePaths(git.commitPaths(expected.evidenceCommit))) !== JSON.stringify([expected.evidencePath])) {
    findings.push(`${expected.id}:evidence-commit-scope`);
  }
  const committedEvidence = git.textAt(expected.evidenceCommit, expected.evidencePath);
  if (canonicalSha256(committedEvidence) !== expected.evidenceCanonicalSha256) findings.push(`${expected.id}:evidence-hash`);
  if (canonicalSha256(git.workingText(expected.evidencePath)) !== expected.evidenceCanonicalSha256) findings.push(`${expected.id}:working-evidence-drift`);
  const normalizedEvidence = canonicalText(committedEvidence);
  for (const fragment of requiredAuthorizationEvidenceFragments()) {
    if (!normalizedEvidence.includes(fragment)) findings.push(`${expected.id}:evidence-content`);
  }
  const child = git.firstParentChild(expected.evidenceCommit);
  if (!EXACT_COMMIT.test(child ?? "") || !git.commitExists(child) || !git.isAncestor(child)) {
    findings.push(`${expected.id}:missing-implementation-child`);
    return [...new Set(findings)];
  }
  if (git.firstParentParent(child) !== expected.evidenceCommit) findings.push(`${expected.id}:implementation-not-immediate`);
  if (JSON.stringify(normalizePaths(git.commitPaths(child))) !== JSON.stringify(normalizePaths(expected.authorizedPaths))) {
    findings.push(`${expected.id}:implementation-child-scope`);
  }
  const beforeContracts = git.jsonAt(expected.evidenceCommit, "PR_CONTRACTS.json");
  let contractBaseline = beforeContracts;
  if (laterRatification) {
    const ratificationFindings = validateLaterContractRatification(git, laterRatification);
    for (const detail of ratificationFindings) findings.push(detail);
    if (ratificationFindings.length === 0) {
      contractBaseline = git.jsonAt(laterRatification.commit, "PR_CONTRACTS.json");
    }
  }
  if (JSON.stringify(normalizeAuthorizedSemanticCorrections(contractBaseline?.prs))
    !== JSON.stringify(normalizeAuthorizedSemanticCorrections(authorizationContracts?.prs))) {
    findings.push(`${expected.id}:ordinary-pr-contract-drift`);
  }
  return [...new Set(findings)];
}

function validateEvidenceRecords(provenance, outcome, findings) {
  const records = provenance?.evidence_records;
  if (!Array.isArray(records) || records.length === 0) {
    findings.push("human-review-evidence-records");
    return;
  }
  let acceptedHumanManualEvidence = false;
  for (const record of records) {
    if (!nonEmptyString(record?.id)
      || !nonEmptyString(record?.source)
      || !nonEmptyString(record?.evidence_ref)
      || !nonEmptyString(record?.verification)
      || !EVIDENCE_DISPOSITIONS.has(record?.reviewer_disposition)) {
      findings.push("human-review-evidence-record");
      continue;
    }
    if (record.source === "user_manual_test"
      && new Set(["accepted_after_review", "independently_verified"]).has(record.reviewer_disposition)) {
      acceptedHumanManualEvidence = true;
    }
    if (outcome === "PASS" && record.reviewer_disposition === "unresolved") findings.push("human-review-unresolved-evidence");
  }
  if (outcome === "PASS" && !acceptedHumanManualEvidence) findings.push("human-review-missing-accepted-user-manual-test");
}

function validateExecutorPreAuthorizations(provenance, outcome, findings) {
  const records = provenance?.executor_pre_authorizations ?? [];
  if (!Array.isArray(records)) {
    findings.push("executor-preauthorization-records");
    return;
  }
  if (outcome === "PASS" && provenance?.pre_authorization_inventory_complete !== true) {
    findings.push("executor-preauthorization-inventory-not-complete");
  }
  for (const record of records) {
    if (!nonEmptyString(record?.authorization_id)
      || !nonEmptyString(record?.scope)
      || !Array.isArray(record?.actions)
      || record.actions.length === 0
      || record.actions.some((action) => !nonEmptyString(action))
      || !nonEmptyString(record?.evidence_ref)
      || !nonEmptyString(record?.recorded_by)
      || !PREAUTH_AUDIT_STATUSES.has(record?.user_audit_status)) {
      findings.push("executor-preauthorization-record");
      continue;
    }
    if (outcome === "PASS" && record.user_audit_status !== "PASS") findings.push("executor-preauthorization-not-user-audited-pass");
  }
}

function validateHumanReviewProvenance(progress, g3, git, findings) {
  const outcome = g3.human_review_status;
  if (!new Set(["PASS", "FAIL"]).has(outcome)) return;
  const provenance = g3.human_review_provenance;
  if (!provenance
    || provenance.generation !== g3.human_review_generation
    || provenance.kind !== "human_manual_test_review"
    || !EXACT_COMMIT.test(provenance.commit ?? "")) {
    findings.push("human-review-provenance");
    return;
  }
  if (provenance.claims_treated_as_untrusted_evidence !== true) findings.push("human-review-trust-policy");
  validateEvidenceRecords(provenance, outcome, findings);
  validateExecutorPreAuthorizations(provenance, outcome, findings);
  const paths = git.commitPaths(provenance.commit);
  if (!git.isAncestor(provenance.commit)
    || !paths
    || paths.length === 0
    || paths.some((path) => !REVIEW_GOVERNANCE_PATHS.has(path.replaceAll("\\", "/")))) {
    findings.push("human-review-decision-commit-scope");
  }
  const before = git.jsonAt(`${provenance.commit}^`, "PR_INDEX.json");
  const after = git.jsonAt(provenance.commit, "PR_INDEX.json");
  const beforeG3 = before?.groups?.find((candidate) => candidate.id === "G3");
  const afterG3 = after?.groups?.find((candidate) => candidate.id === "G3");
  const afterG4 = after?.groups?.find((candidate) => candidate.id === "G4");
  const afterLb18 = after?.prs?.find((candidate) => candidate.id === "LB-018");
  if (beforeG3?.status !== "PASS"
    || beforeG3?.review_status !== "PASS"
    || beforeG3?.human_review_status !== "REQUIRED"
    || beforeG3?.human_review_generation !== provenance.generation
    || afterG3?.human_review_status !== outcome) {
    findings.push("human-review-decision-transition");
  }
  if (outcome === "PASS") {
    if (afterG3?.status !== "PASS"
      || afterG3?.review_status !== "PASS"
      || afterG4?.status !== "READY"
      || afterLb18?.status !== "READY"
      || after?.execution?.current_group !== "G4"
      || after?.execution?.current_pr !== "LB-018") {
      findings.push("human-review-pass-unlock-transition");
    }
  } else {
    const reopenedId = after?.execution?.current_pr;
    const reopenedPr = after?.prs?.find((candidate) => candidate.id === reopenedId);
    if (afterG3?.status !== "REWORK_REQUIRED"
      || afterG3?.review_status !== "REQUIRED"
      || afterG4?.status !== "BLOCKED"
      || afterLb18?.status !== "BLOCKED"
      || after?.execution?.current_group !== "G3"
      || !g3.prs?.includes(reopenedId)
      || reopenedPr?.status !== "REWORK_REQUIRED") {
      findings.push("human-review-fail-reopen-transition");
    }
  }
}

export function validateG4HumanGate(progress, git) {
  const findings = [];
  const groups = progress?.groups ?? [];
  const prs = progress?.prs ?? [];
  const g3 = groups.find((candidate) => candidate.id === "G3");
  const g4 = groups.find((candidate) => candidate.id === "G4");
  const lb18 = prs.find((candidate) => candidate.id === "LB-018");
  if (!g3 || !g4 || !lb18) return findings;
  const humanStatus = g3.human_review_status ?? "BLOCKED";
  const humanGeneration = g3.human_review_generation ?? 0;
  if (!HUMAN_REVIEW_STATUSES.has(humanStatus) || !Number.isInteger(humanGeneration) || humanGeneration < 0) {
    findings.push("human-review-state");
    return findings;
  }
  const g4Unlocked = g4.status !== "BLOCKED" || lb18.status !== "BLOCKED";
  if (g4Unlocked && !(g3.status === "PASS" && g3.review_status === "PASS" && humanStatus === "PASS")) {
    findings.push("g4-unlocked-without-double-pass");
  }
  if (humanStatus === "BLOCKED" && g3.status === "PASS" && g3.review_status === "PASS") {
    findings.push("g3-pass-without-required-human-gate");
  }
  if (humanStatus === "REQUIRED") {
    if (g3.status !== "PASS"
      || g3.review_status !== "PASS"
      || g4.status !== "BLOCKED"
      || lb18.status !== "BLOCKED"
      || progress?.execution?.current_group !== "G3"
      || progress?.execution?.current_pr !== null) {
      findings.push("human-review-required-state");
    }
  }
  if (humanStatus === "PASS" && (g3.status !== "PASS" || g3.review_status !== "PASS")) findings.push("human-review-pass-without-g3-pass");
  if (humanStatus === "FAIL") {
    const reopenedId = progress?.execution?.current_pr;
    const reopenedPr = prs.find((candidate) => candidate.id === reopenedId);
    if (g3.status !== "REWORK_REQUIRED"
      || g3.review_status !== "REQUIRED"
      || g4.status !== "BLOCKED"
      || lb18.status !== "BLOCKED"
      || progress?.execution?.current_group !== "G3"
      || !g3.prs?.includes(reopenedId)
      || reopenedPr?.status !== "REWORK_REQUIRED") {
      findings.push("human-review-fail-state");
    }
  }
  if (g3.review_status === "PASS" && g3.review_provenance?.kind === "independent_adversarial" && EXACT_COMMIT.test(g3.review_provenance?.commit ?? "")) {
    const afterReview = git.jsonAt(g3.review_provenance.commit, "PR_INDEX.json");
    const afterG3 = afterReview?.groups?.find((candidate) => candidate.id === "G3");
    const afterG4 = afterReview?.groups?.find((candidate) => candidate.id === "G4");
    const afterLb18 = afterReview?.prs?.find((candidate) => candidate.id === "LB-018");
    if (afterG3?.human_review_status !== "REQUIRED"
      || !Number.isInteger(afterG3?.human_review_generation)
      || afterG3.human_review_generation < 1
      || afterG4?.status !== "BLOCKED"
      || afterLb18?.status !== "BLOCKED"
      || afterReview?.execution?.current_group !== "G3"
      || afterReview?.execution?.current_pr !== null) {
      findings.push("g3-adversarial-pass-transition-bypassed-human-gate");
    }
  }
  validateHumanReviewProvenance(progress, g3, git, findings);
  return [...new Set(findings)];
}
