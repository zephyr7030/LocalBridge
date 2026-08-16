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

const G3_MANUAL_REVIEW_ROUND2_2026_08_14 = Object.freeze({
  schemaVersion: 23,
  baselineCommit: "a9b7c20c6ee103ed8048bd2dd9764af1920fcc6e",
  baselineSchemaVersion: 22,
  replacedRules: {
    dashboard_current_task_status: ["current_status_plus_single_last_tool_row", "single_current_or_last_timing_projection"],
    onboarding_window_default_inner_size: [[780, 620], [900, 620]],
    onboarding_window_min_inner_size: [[780, 620], [900, 620]],
    onboarding_window_max_inner_size: [[780, 620], [900, 620]],
  },
  addedRules: {
    task_backend_wakeup_delivery_required: true,
    task_frontend_polling_as_short_task_transport_forbidden: true,
    task_minimum_visible_duration_ms: 500,
    task_minimum_visibility_must_not_delay_tool_response: true,
    task_last_tool_row_required: true,
    task_last_tool_row_label: "上次执行工具：",
    task_last_tool_age_right_aligned: true,
    task_idle_age_moved_to_last_tool_row: true,
    task_last_tool_label_secret_redacted: true,
    task_last_tool_raw_mcp_identifier_forbidden: true,
    settings_runtime_api_key_clear_action_required: true,
    settings_runtime_api_key_clear_label: "清除",
    settings_runtime_api_key_clear_immediately_left_of_replace: true,
    settings_runtime_api_key_clear_deletes_secure_credential: true,
    settings_runtime_api_key_clear_never_reveals_secret: true,
    settings_runtime_api_key_clear_reuses_controlled_connection_change_lifecycle: true,
    scrollable_rounded_surface_preserves_outer_corners: true,
    scrollbar_must_be_clipped_or_inset_inside_rounded_surface: true,
    onboarding_two_line_button_min_rendered_height_multiplier: 2,
    onboarding_two_line_button_computed_geometry_gate_required: true,
    onboarding_two_line_button_css_marker_only_pass_forbidden: true,
    onboarding_saved_runtime_key_status_text: "已安全保存至windows安全凭据",
    onboarding_saved_runtime_key_mask_matches_saved_secret_length: true,
    onboarding_saved_runtime_key_mask_is_display_only: true,
    onboarding_saved_runtime_key_mask_must_never_be_submitted_as_secret: true,
    onboarding_runtime_key_length_metadata_only_allowed: true,
    onboarding_saved_tunnel_id_prefill_required: true,
    onboarding_runtime_key_security_hint: "Runtime API Key 仅保存在 Windows 安全凭据中。",
  },
  lb015: {
    addedWritablePaths: ["src-tauri/src/tray/**", "src-tauri/src/main.rs"],
    artifactReplacements: [[
      "backend wake-driven CurrentTask presentation with elapsed timing, minimum 500ms visibility, and one retained last-tool row so short real executions cannot be missed",
      "backend-only CurrentTask truth with elapsed timing and retained last-command time metadata so short real executions cannot be missed",
    ]],
    addedArtifacts: [
      "Settings Runtime API Key secure credential clear action immediately left of replace",
      "rounded outer UI surfaces that preserve all corners while inner content scrolls",
      "fixed 780x620 non-resizable non-maximizable main window shell",
    ],
    testReplacements: [
      ["Dashboard no-task first row is always visible as 等待命令 and never 空闲; relative age is not appended to this first row", "Dashboard no-task state is always visible as 等待命令 and never 空闲; after at least one real task it also shows backend-grounded last-command relative age"],
      ["real production MCP and Broker execution wakes the Dashboard through a backend push/event or equivalent wakeup path rather than waiting for the periodic projection poll", "real production MCP and Broker execution transitions backend CurrentTaskStatus to the Dashboard, including executions completing faster than the frontend refresh interval, and active projection includes elapsed duration"],
      ["every real tool call including create modify delete and ordinary command remains visibly represented for at least 500ms even when execution completes faster, without delaying the actual tool response solely for UI visibility", "create modify delete and ordinary command tool calls each update backend current/last task timing truth rather than depending on frontend polling luck"],
      ["task age formats on the last-tool row cover nS前, n分钟前, 大于1小时 and 大于n天 while retaining only single last-tool metadata and no history/feed/list", "task age formats cover nS前, n分钟前, 大于1小时 and 大于n天 while retaining no task history/feed/list"],
      ["Settings Tunnel ID and Runtime API Key 更换 buttons remain on the same right-side action column within 1 CSS px at 780x620; when Runtime API Key is saved a 清除 button is immediately to the left of its 更换 button", "Settings Tunnel ID and Runtime API Key 更换 buttons share the same action-column left edge within 1 CSS px at 900x620 regardless of summary text width"],
    ],
    addedTests: [
      "bursty short tool calls each receive the minimum visible presentation interval without creating a user-browsable history/feed/list",
      "a second row displays 上次执行工具： followed by a secret-redacted user-facing tool label or safe summary and places the backend-grounded relative age at the far right",
      "Settings Runtime API Key 清除 deletes the saved Windows secure credential without revealing it, updates the projection to 未保存, and applies the existing controlled connection-change lifecycle when services are active or connecting",
      "main window default minimum and maximum inner size are all exactly 780x620 while resizable false maximizable false decorations false and the existing custom chrome semantics remain unchanged",
      "forcing Settings or another rounded sheet/dialog to overflow vertically at 780x620 preserves all four outer rounded corners; the scrollbar is clipped or inset inside the rounded shell and never flattens or cuts the outer radius",
    ],
  },
  lb016: {
    artifactReplacements: [["fixed 780x620 non-resizable non-maximizable main window", "fixed 900x620 non-resizable non-maximizable main window"]],
    testReplacements: [
      ["screen 2 shows exactly the one-line hint Runtime API Key 仅保存在 Windows 安全凭据中。 with no trailing storage explanation", "screen 2 shows one-line secure Runtime API Key storage hint"],
      ["screen 3 permission mode buttons that contain title plus description render at least twice the actual rendered height of the ordinary single-line control at 780x620; both text line boxes are fully visible and final visual PASS requires human inspection rather than CSS marker presence", "screen 3 permission mode buttons preserve clearly visible balanced content-to-border spacing in the real 900x620 render and grow safely for wrapped descriptive text; final visual PASS requires human inspection and cannot be inferred from CSS padding markers alone"],
      ["main window is fixed to 780x620 with minimum and maximum 780x620 resizable false and maximizable false", "main window is fixed to 900x620 with minimum and maximum 900x620 resizable false and maximizable false"],
      ["screen 3 two-line permission buttons pass a computed/rendered geometry Gate proving actual height at least 2x a single-line control and complete title/description line boxes; human 780x620 visual Gate remains required and static min-height CSS alone cannot PASS", "screen 3 permission buttons have structural min-height at least 80px or an equivalent provable layout for title plus two-line description and balanced vertical whitespace; human 900x620 visual Gate remains required"],
    ],
    addedTests: [
      "screen 2 pre-fills the currently persisted Tunnel ID when one is already saved",
      "screen 2 saved Runtime API Key state uses the exact text 已安全保存至windows安全凭据",
      "when a saved Runtime API Key field is focused it shows a display-only asterisk mask with exactly the saved key character count using length metadata only; plaintext is never returned to the frontend and an untouched mask is never submitted or saved as a replacement key",
    ],
  },
});

const LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14 = Object.freeze({
  schemaVersion: 25,
  baselineCommit: "667e89f037e39afc9c9c4f1df4999f3b2213dffe",
  baselineSchemaVersion: 24,
  addedRules: {
    localbridge_agent_api_owned: true,
    localbridge_agent_api_versioned: true,
    upstream_tools_list_public_passthrough_forbidden: true,
    upstream_tool_schema_public_leak_forbidden: true,
    upstream_tool_private_error_leak_forbidden: true,
    upstream_new_tools_auto_publication_forbidden: true,
    localbridge_agent_api_v1_core_tools: ["workspace_context", "agent_workflow", "exec_command", "command_control", "task_control", "git_workflow", "document_workflow", "view_image"],
    localbridge_agent_api_privileged_extensions: ["elevated_exec"],
    elevated_exec_conditional_broker_policy_preserved: true,
    public_tools_list_policy_filtered_subset_of_registry_required: true,
    runtime_capability_negotiation_required: true,
    runtime_missing_required_capability_fail_closed: true,
    runtime_adapter_result_error_normalization_required: true,
    policy_classifies_localbridge_capabilities_not_upstream_names: true,
    workflow_transitive_capability_declaration_required: true,
    upstream_runtime_internal_replaceable_backend: true,
    execution_envelope_localbridge_tool_identity_required: true,
    shell_resolver_owned_by_localbridge: true,
    shell_selector_values: ["auto", "powershell", "pwsh", "windows_powershell", "cmd"],
    shell_auto_preference: ["trusted_highest_powershell_core", "windows_powershell_5_1", "cmd"],
    shell_text_guessing_forbidden: true,
    shell_arbitrary_executable_from_mcp_forbidden: true,
    shell_auto_install_or_update_forbidden: true,
    shell_default_candidate_requires_trusted_install_or_explicit_registered_identity: true,
    shell_path_discovery_is_not_trust_authority: true,
    shell_probe_requires_candidate_trust_validation: true,
    shell_version_selection_semantic_required: true,
    direct_process_and_shell_execution_separated: true,
    custom_shell_registry_v0_1_required: false,
    wsl_container_remote_shell_v0_1_required: false,
    environment_manager_abstraction_v0_1_required: false,
  },
  lb006: {
    addedWritablePaths: ["tests/unit/mcp/**", "tests/integration/command/**"],
    addedArtifacts: [
      "LocalBridge-owned versioned public ToolRegistry and Agent Runtime facade",
      "replaceable internal WorkspaceRuntimeAdapter or equivalent package adapter boundary",
      "runtime capability negotiator for mandatory LocalBridge facade capabilities",
      "LocalBridge ShellResolver using structured logical shell selectors",
      "separate DirectProcessExecutor and ShellExecutor execution paths",
      "stable LocalBridge result and typed-error normalization adapter",
    ],
    addedTests: [
      "public ToolRegistry defines exactly the eight LocalBridge v1 non-privileged core tools; tools/list returns only the current-policy-eligible subset plus policy-eligible LocalBridge privileged extensions and never upstream private tool names",
      "adding a fake new upstream tool or changing an upstream private tool schema does not change the public LocalBridge tool registry",
      "missing any mandatory LocalBridge facade runtime capability or incompatible adapter schema fails closed before serving the facade",
      "upstream private result and error shapes are normalized into stable LocalBridge result/error contracts without leaking private schema details",
      "ShellResolver auto selects the highest compatible trusted PowerShell Core by semantic version then Windows PowerShell 5.1 then cmd",
      "a malicious earlier PATH pwsh.exe is rejected without execution or version probing unless its executable identity independently satisfies the trusted candidate policy",
      "logical selectors auto powershell pwsh windows_powershell cmd resolve without command-text guessing and MCP cannot supply an arbitrary shell executable path",
      "ShellResolver never automatically installs or updates a shell",
      "direct process execution and shell execution remain structurally separate and use structured specifications",
    ],
    addedNonGoals: [
      "WSL container or remote shell backends in v0.1",
      "custom shell registry in v0.1",
      "environment-manager abstraction in v0.1",
      "freezing the exact internal source directory layout",
    ],
  },
  lb007: {
    addedArtifacts: [
      "LocalBridge stable public capability/action classifier independent of upstream tool names",
      "transitive capability declarations for high-level LocalBridge workflows",
    ],
    addedTests: [
      "PEP classifies stable LocalBridge public actions and capabilities rather than raw upstream tool names",
      "raw upstream tool names cannot be called as a public bypass around the LocalBridge facade",
      "unknown LocalBridge public actions or capabilities fail closed",
      "high-level workflows declare and enforce all transitive write process network and privilege capabilities before execution",
      "cached or stale public tools/list cannot bypass a later permission-mode or capability-policy change",
    ],
  },
});

const ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14 = Object.freeze({
  schemaVersion: 26,
  baselineSchemaVersion: 25,
  replacedRules: {
    ui_admin_mode_logic_accent: ["#ff9500", "amber"],
  },
  removedBaselineRules: {
    visible_admin_mode_selection_requests_uac: true,
  },
  addedRules: {
    ui_admin_mode_logic_color_name: "orange",
    admin_mode_warning_surfaces: ["settings", "onboarding_screen_3"],
    visible_admin_mode_selection_opens_warning_gate: true,
    admin_mode_warning_required_before_uac: true,
    admin_mode_warning_intro: "启用管理员权限后，错误或恶意操作可能导致：",
    admin_mode_warning_consequences: [
      "删除或覆盖重要文件",
      "修改系统关键配置",
      "软件或系统无法正常启动",
      "数据永久丢失",
      "安全机制被绕过或关闭",
      "凭据、密钥等敏感信息泄露",
      "恶意程序获得更高权限",
      "系统被破坏，严重时可能需要重装 Windows",
    ],
    admin_mode_warning_footer: "仅在你明确理解操作后果时授权。",
    admin_mode_warning_cancel_label: "取消",
    admin_mode_warning_confirm_button_color: "red",
    admin_mode_warning_confirm_entire_button_red: true,
    admin_mode_warning_countdown_ms: 9000,
    admin_mode_warning_countdown_labels: ["确认9", "确认8", "确认7", "确认6", "确认5", "确认4", "确认3", "确认2", "确认1"],
    admin_mode_warning_confirm_ready_label: "确认",
    admin_mode_warning_confirm_disabled_during_countdown: true,
    admin_mode_warning_countdown_monotonic_elapsed_required: true,
    admin_mode_warning_countdown_bypass_forbidden: true,
    admin_mode_warning_cancel_escape_dismiss_no_side_effect: true,
    admin_mode_warning_fresh_open_restarts_countdown: true,
    admin_mode_warning_remember_or_skip_forbidden: true,
    admin_mode_uac_before_enabled_confirm_forbidden: true,
    admin_mode_background_restore_warning_forbidden: true,
    admin_mode_active_broker_reselection_duplicate_uac_forbidden: true,
    admin_mode_warning_is_narrow_safety_dialog_not_wizard_shell: true,
    admin_mode_warning_ai_mcp_approval_forbidden: true,
    admin_mode_warning_separate_from_high_critical_operation_confirmation: true,
  },
  lb015: {
    artifactReplacements: [[
      "blue #0071e3 standard product accent with orange #ff9500 administrator-mode warning exception",
      "blue #0071e3 standard product accent with amber administrator-mode exception",
    ]],
    testReplacements: [
      [
        "administrator mode controls in onboarding and Settings use orange #ff9500 warning styling and are not overridden by ordinary blue selected styling",
        "administrator mode uses amber logical selection styling and is not overridden by ordinary blue selected styling",
      ],
      [
        "Settings has no separate 启用管理员权限 button; when Broker is not Active, visible 管理员模式 selection or reselection opens the safety warning gate and cannot request UAC before the enabled red 确认 action after the full 9000ms countdown",
        "Settings has no separate 启用管理员权限 button; visible 管理员模式 selection or reselection in Settings requests UAC when privilege is not Active",
      ],
    ],
    addedTests: [
      "administrator warning uses exact intro, eight ordered consequence bullets, and footer from schema26 with actions 取消 and one whole-red confirmation button",
      "administrator warning red confirmation button is disabled for the full monotonic 9000ms interval, labels 确认9 through 确认1, then becomes enabled with exact label 确认; pointer keyboard synthetic repeated click rerender focus change or stale frontend state cannot bypass the gate",
      "administrator warning cancel Escape close or dismiss causes no PermissionMode Broker or UAC side effect; every fresh open restarts the full nine-second countdown and no remember skip or do-not-show-again bypass exists",
      "background administrator preference restore shows no safety dialog and requests no UAC; reselecting an already-active administrator mode causes no duplicate UAC",
      "mode-entry warning does not approve any separate High or Critical per-operation confirmation and AI/MCP cannot approve the warning or mutate PermissionMode Broker or other control-plane authority",
    ],
  },
  lb016: {
    artifactReplacements: [[
      "administrator-mode orange #ff9500 warning plus whole-red nine-second confirmation gate before UAC activation",
      "administrator-mode selection UAC activation",
    ]],
    testReplacements: [
      [
        "ordinary selected permission modes use the standard blue accent while administrator mode uses orange #ff9500 warning styling in onboarding and Settings",
        "ordinary selected permission modes use the standard blue accent while administrator mode uses amber logical styling in onboarding and Settings",
      ],
      [
        "screen 3 visible selection or reselection of 管理员模式 opens the fixed safety warning when Broker is not Active; no UAC or permission activation may occur until the whole-red confirmation button completes its full 9000ms disabled countdown and the user clicks enabled 确认; no separate enable-admin button exists",
        "screen 3 visible selection or reselection of 管理员模式 is the explicit user action that requests UAC when broker is not Active; no separate enable-admin button exists",
      ],
      [
        "background preference restore shows neither the administrator warning nor UAC; a fresh visible warning always restarts the full nine-second countdown",
        "background preference restore remains non-UAC even though visible administrator-mode selection requests UAC",
      ],
    ],
    addedTests: [
      "administrator warning is a narrow safety consent dialog and does not permit the five-screen onboarding shell to regress into a centered card modal or dialog layout",
    ],
  },
});

const PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16 = Object.freeze({
  schemaVersion: 34,
  baselineSchemaVersion: 33,
  removedRules: {
    permission_mode_full_workspace_bound: true,
  },
  addedRules: {
    permission_mode_edit_structured_workspace_only: true,
    permission_mode_edit_ordinary_shell_process_forbidden: true,
    permission_mode_full_structured_inputs_workspace_bound: true,
    permission_mode_full_workdir_workspace_bound: true,
    permission_mode_full_explicit_path_arguments_workspace_bound: true,
    permission_mode_full_ordinary_shell_process_allowed: true,
    permission_mode_full_ordinary_process_token: "current_windows_user",
    permission_mode_full_shell_child_os_workspace_isolation_guaranteed: false,
    permission_mode_full_shell_child_os_access_scope: "current_windows_user_token",
    permission_mode_elevated_ordinary_route_token: "current_windows_user",
    permission_mode_elevated_administrator_route: "elevated_exec_broker_uac",
    policy_denial_typed_error_projection_required: true,
    full_workspace_script_extension_alone_not_denied: true,
    dynamic_privileged_tool_catalog_refresh_required: true,
    privileged_tool_stale_catalog_call_fail_closed: true,
  },
  lb006: {
    artifactReplacements: [
      ["mode-aware LocalBridge structured path/workdir authority adapter: Edit and Full structured document image Git file edit directory and exec workdir/typed path inputs remain active-workspace-bound; this boundary does not claim OS-level filesystem confinement for Full child processes, while Elevated administrator paths are Broker-backed", "mode-aware public path/workdir authority adapter: Edit and Full are active-workspace-relative while Elevated privileged operations may address administrator-token-scoped filesystem paths outside the active workspace"],
      ["permission-scope-aware Git repository resolver: LocalBridge ordinary Git routing remains bounded to the active workspace in Edit Full and ordinary Elevated routes; administrator-token outside-workspace authority is available only through the separate Broker-backed privileged route", "permission-scope-aware Git repository resolver: Edit and Full stop at the active workspace root while Elevated may resolve administrator-token-accessible repositories outside that root"],
    ],
    testReplacements: [
      ["Edit and Full LocalBridge structured document image Git file edit directory and exec workdir/typed path inputs remain active-workspace-relative and reject drive UNC verbatim POSIX absolute or parent traversal; this structured boundary does not sandbox Full child-process filesystem access, while Elevated privileged routes accept administrator-token-accessible absolute paths", "Edit and Full document image Git and exec workdir inputs remain active-workspace-relative and reject drive UNC verbatim POSIX absolute or parent traversal; Elevated privileged filesystem and administrator execution routes accept administrator-token-accessible absolute paths outside the active workspace without weakening Edit or Full"],
      ["ordinary Git repository discovery never escapes the active workspace in Edit Full or ordinary Elevated routing; administrator-token outside-workspace repository authority requires the separate Broker-backed privileged route, and git_diff never uses non-git fallback once the LocalBridge resolver has confirmed a repository", "Git repository discovery never escapes the active workspace in Edit or Full; Elevated may resolve repositories outside the active workspace only through its administrator-token scope, and git_diff never uses non-git fallback once the LocalBridge resolver has confirmed a repository"],
      ["Elevated path authority is mode-aware rather than a lexical workspace bypass: LocalBridge structured ordinary routes stay workspace-bound, Full child-process filesystem authority remains the current ordinary-user token without an OS workspace-sandbox promise, and administrator-token operations outside the active workspace are dispatched only through a Broker-backed privileged filesystem or administrator execution route", "Elevated path authority is mode-aware rather than a lexical workspace bypass: the ordinary user route stays workspace-bound, while operations outside the active workspace are dispatched only through a Broker-backed privileged filesystem or administrator execution route and are bounded by the administrator token"],
    ],
    addedTests: [
      "Edit exposes and authorizes no ordinary shell or process execution, including direct exec_command calls, stale cached calls and agent_workflow command indirection; denial occurs before any process launch",
      "Full allows trusted cmd PowerShell and development process execution only under the current Windows ordinary-user token; the ordinary route never receives or inherits Broker administrator authority",
      "Full child-process filesystem access is governed by the current Windows ordinary-user token and LocalBridge makes no OS-level active-workspace sandbox guarantee for those descendants; outside-workspace child access permitted by that token is not itself a contract defect while LocalBridge structured path and workdir inputs remain active-workspace-bound",
    ],
  },
  lb007: {
    addedArtifacts: [
      "stable public typed policy-denial error projection",
      "protocol-correct privileged tool catalog lifecycle across permission and Broker transitions",
    ],
    testReplacements: [["Edit and Full cannot obtain administrator-token filesystem process command or system-maintenance capability through workflow indirection; Full ordinary process descendants retain only the current Windows ordinary-user token, Elevated may declare privileged capabilities only for the Broker-backed administrator route, and LocalBridge control-plane remains deny-always", "Edit and Full cannot obtain administrator-token filesystem process command or system-maintenance capability through workflow indirection; Elevated may declare those privileged capabilities only for the Broker-backed administrator route and LocalBridge control-plane remains deny-always"]],
    addedTests: [
      "public MCP policy denial preserves stable typed LocalBridge reason categories so capability or policy denial workspace denial and privileged-route or elevation requirements do not collapse into one generic JSON-RPC denial",
      "Full ordinary invocation of a validated active-workspace .ps1 .cmd or .bat script is not denied solely because of its file extension; dynamic script resolution unsafe indirection provider mutation and other independently review-required behavior remain fail-closed",
      "an already-connected MCP session cannot remain permanently unaware of a permission or Broker capability change: entering Elevated plus Active Broker causes a protocol-correct tools/list refresh notification or controlled reconnect, leaving Elevated revokes the privileged catalog capability, and stale calls are still re-authorized fail-closed",
    ],
  },
  lb012: {
    addedArtifacts: ["privileged tool publication and revocation lifecycle for already-connected MCP sessions"],
    testReplacements: [["Elevated plus Active Broker can read create modify rename and delete administrator-token-accessible filesystem objects outside the active workspace through a privileged filesystem route; Edit structured operations and Full LocalBridge structured filesystem/path tools remain active-workspace-bound, while Full ordinary child-process filesystem access is governed by the current Windows ordinary-user token and is outside LocalBridge's OS-level workspace-sandbox guarantee", "Elevated plus Active Broker can read create modify rename and delete administrator-token-accessible filesystem objects outside the active workspace through a privileged filesystem route; Edit and Full remain unable to cross the active workspace authorization root"]],
    addedTests: ["an already-connected MCP session that enters Elevated and reaches Broker Active reliably receives or is forced through a protocol-correct tool-capability refresh or reconnect so elevated_exec becomes discoverable and callable; leaving Elevated revokes or denies it, a stale catalog call fails closed, and ordinary exec_command remains current-user throughout"],
  },
});

const PERMISSION_MODE_EQUAL_THIRDS_GEOMETRY_AMENDMENT_2026_08_16 = Object.freeze({
  schemaVersion: 35,
  baselineSchemaVersion: 35,
  addedRules: {
    ui_permission_mode_three_way_group_content_size_exempt: true,
    ui_permission_mode_three_way_group_labels: ["编辑模式", "完整模式", "管理员模式"],
    ui_permission_mode_three_way_group_layout: "symmetric_equal_three_columns",
    ui_permission_mode_three_way_group_equal_width_required: true,
    ui_permission_mode_three_way_group_equal_height_required: true,
    ui_permission_mode_three_way_group_text_clipping_forbidden: true,
  },
  lb015: {
    testReplacements: [[
      "every text-bearing button except the symmetric 编辑模式/完整模式/管理员模式 three-way PermissionMode selection group sizes from its own content metrics: width corresponds to the maximum visible character count on any single line and height corresponds to the actual rendered line count; arbitrary fixed button geometry that ignores text content is forbidden; the three-way PermissionMode group is explicitly exempt from content-derived width/height sizing and instead must render as exactly three symmetric equal-width equal-height cells with no clipped or overflowing text",
      "every text-bearing button sizes from its own content metrics: width corresponds to the maximum visible character count on any single line and height corresponds to the actual rendered line count; arbitrary fixed button geometry that ignores text content is forbidden",
    ]],
  },
  lb016: {
    testReplacements: [
      [
        "screen 3 编辑模式 完整模式 管理员模式 permission buttons are an explicit symmetric three-way-group exception to generic content-derived button sizing: they render as exactly three equal-width equal-height cells in a symmetric layout; title and description content must remain fully visible without clipping or overflow and final visual PASS requires human inspection rather than CSS marker presence",
        "screen 3 text-bearing permission buttons derive width from the maximum visible character count on any single line and height from the actual title plus description rendered line count; content must not be clipped and final visual PASS requires human inspection rather than CSS marker presence",
      ],
      [
        "screen 3 permission buttons pass a computed/rendered geometry Gate proving exactly three symmetric equal-width equal-height cells with complete title and description line boxes and no clipping or overflow; this PermissionMode group is exempt from generic content-derived width/height sizing and human 780x620 visual Gate remains required",
        "screen 3 permission buttons pass a computed/rendered geometry Gate proving geometry follows content metrics rather than a fixed min-height or multiplier; title and description line boxes are complete and human 780x620 visual Gate remains required",
      ],
    ],
  },
});

const PUBLIC_MCP_OUTPUT_SCHEMA_AMENDMENT_2026_08_16 = Object.freeze({
  schemaVersion: 35,
  baselineSchemaVersion: 35,
  lb006: {
    addedArtifacts: [
      "LocalBridge-owned MCP output schemas for every advertised public tool, matching the stable public structuredContent contract without exposing upstream private output schemas",
    ],
    addedTests: [
      "every advertised LocalBridge public tool including privileged extensions declares a non-empty LocalBridge-owned outputSchema matching its actual structuredContent; agent_workflow describes the stable ok/data/error envelope and action/state/workspace/project/commands result fields, elevated_exec describes its existing privileged result variants, and upstream private outputSchema is never exposed directly",
    ],
  },
});

const G3_UI_TRAY_REFINEMENT_AMENDMENT_2026_08_16 = Object.freeze({
  schemaVersion: 35,
  baselineSchemaVersion: 35,
  replacedRules: {
    dashboard_permission_mode_row_forbidden: { current: false, baseline: true },
    dashboard_admin_privilege_status_read_only: { current: false, baseline: true },
    onboarding_screen_4_plugin_management_guidance: {
      current: "打开新建插件页后，选择隧道并选择刚刚添加的Tunel，创建插件",
      baseline: "打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件",
    },
  },
  addedRules: {
    dashboard_permission_mode_read_only_row_required: true,
    dashboard_permission_mode_row_label: "权限模式",
    dashboard_permission_mode_row_values: ["编辑模式", "完整模式", "管理员模式"],
    dashboard_permission_mode_status_dot_forbidden: true,
    ui_panel_base_color_difference_required: false,
    ui_flat_primary_surface_allowed: true,
    ui_fake_elevation_through_near_invisible_surface_treatment_forbidden: true,
    onboarding_screen_4_new_connector_action_label: "打开新建插件页",
    onboarding_screen_4_new_connector_action_before_information_rows: true,
    onboarding_screen_4_information_label_column_aligned: true,
    onboarding_screen_4_copy_action_right_edge_aligned: true,
    tray_icon_ico: "assets/icons/localbridge-tray.ico",
    tray_icon_required_frame_sizes: [16, 20, 24, 32, 48],
    tray_icon_small_frame_simplified_design_required: false,
    tray_icon_runtime_resampling_forbidden: true,
    tray_icon_exact_dpi_frame_mapping: { "1.0": 16, "1.25": 20, "1.5": 24, "2.0": 32 },
    tray_icon_large_brand_art_direct_use_forbidden: true,
    windows_icon_master_png: "assets/icons/localbridge.png",
    windows_icon_master_png_sha256: "710690f2d70e3c69f13db9d4eaebc0bef5c80561c74acc7bc5a401c15c16e55a",
    windows_taskbar_icon_ico: "assets/icons/localbridge.ico",
    windows_taskbar_icon_sha256: "c995d6af01ebc5031950eb9ea6415b58671b31f84ed6b55baabe80ea51e33f78",
    windows_taskbar_icon_master_png_derivation_required: true,
    tray_icon_master_png_derivation_required: true,
    tray_icon_new_authored_graphics_forbidden: true,
    tray_icon_allowed_processing: ["crop", "downscale", "ico-packaging"],
    tray_icon_source_crop_rect: { x: 90, y: 400, width: 390, height: 390 },
    tray_icon_derived_sha256: "5a9fce6e80050c9ce1620b8e28767052ece2536213ee04fe532596d3f4ec811d",
    dashboard_project_scope_display_source: "permission_mode",
    dashboard_project_scope_display_privilege_state_independent: true,
    dashboard_project_scope_by_permission_mode: { edit: "active_workspace_path", full: "active_workspace_path", admin: "全目录访问" },
    dashboard_project_switch_blocked_in_admin_mode: true,
  },
  prs: {
    "LB-013": {
      addedWritablePaths: ["assets/icons/localbridge-tray.ico", "scripts/icons/derive-tray-icon.ps1"],
      artifactReplacements: [[
        "Windows taskbar and notification-area tray icons derive only from frozen assets/icons/localbridge.png; the accepted clear taskbar localbridge.ico remains unchanged while tray uses a PNG crop/downscale multi-resolution ICO",
        "system tray uses frozen LocalBridge brand icon",
      ]],
      testReplacements: [[
        "tray icon is reproducibly derived only from frozen assets/icons/localbridge.png crop 90,400,390,390 using crop/downscale/ICO packaging into dedicated 16/20/24/32/48 frames with exact 100/125/150/200% DPI selection and no runtime resampling",
        "tray icon derives from frozen localbridge.ico asset",
      ]],
      addedTests: [
        "tray 16/20/24/32/48 frames contain only pixels derived from the original LocalBridge PNG crop/downscale path; newly authored glyphs backgrounds symbols or third-party-lookalike artwork are forbidden",
        "Windows taskbar/left-bottom icon remains the accepted frozen assets/icons/localbridge.ico and both taskbar and tray icon provenance is anchored to assets/icons/localbridge.png",
      ],
    },
    "LB-015": {
      artifactReplacements: [[
        "Dashboard read-only PermissionMode status",
        "Dashboard privilege runtime status",
      ]],
      testReplacements: [
        ["Dashboard 权限模式 row displays exactly 编辑模式 完整模式 or 管理员模式 from backend PermissionMode", "Dashboard shows privilege status independently from permission preference"],
        ["Dashboard 权限模式 display is sourced from backend PermissionMode while Settings and onboarding administrator activation still use PrivilegeState for runtime elevation state", "Dashboard read-only administrator privilege status plus Settings and onboarding privilege runtime state are sourced from PrivilegeState rather than permission preference alone"],
        ["Dashboard renders exactly one read-only 权限模式 row showing 编辑模式 完整模式 or 管理员模式 and renders no permission selection controls", "Dashboard renders no 权限模式 row and no 编辑模式 完整模式 管理员模式 selection controls"],
        ["Dashboard 权限模式 row is read-only and cannot mutate PermissionMode or request UAC", "Dashboard administrator privilege status is read-only and sourced from PrivilegeState"],
        ["Dashboard current-project scope is bound only to backend PermissionMode rather than Broker/PrivilegeState: Edit and Full display the active-workspace path, while Admin displays yellow 全目录访问 for the entire time Admin is the selected mode; Admin project switching remains blocked and shows exactly 管理员模式拥有系统管理员令牌范围内的文件访问能力，若要切换，请切换其他模式", "when PermissionMode is Elevated and Broker is Active the Dashboard current-project area displays yellow 全目录访问 instead of implying an active-workspace access boundary; clicking the project-switch affordance does not switch workspace and shows exactly 管理员模式拥有系统管理员令牌范围内的文件访问能力，若要切换，请切换其他模式"],
      ],
      addedTests: [
        "Dashboard 权限模式 row has no service/status indicator dot and its visible value comes directly from backend PermissionMode",
        "Dashboard main data surface is intentionally flat: card/base color contrast border and drop-shadow are not required and near-invisible fake elevation styling is absent",
      ],
    },
    "LB-016": {
      testReplacements: [
        ["screen 4 shows 打开新建插件页后，选择隧道并选择刚刚添加的Tunel，创建插件 beneath the plugin-settings action", "screen 4 shows 打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件 beneath the plugin-settings action"],
        ["screen 4 new-connector action label is 打开新建插件页", "screen 4 lower plugin action label is 打开插件管理页"],
        ["screen 4 打开新建插件页 action is placed immediately above the 名称/Tunnel ID information rows in the left-side action flow", "screen 4 lower plugin-management action is placed in the left-side action flow"],
      ],
      addedTests: [
        "screen 4 名称 and Tunnel ID rows share one aligned left label column/value start and their copy actions share one aligned right edge without fixed copy-button width",
      ],
    },
  },
});

const UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16 = Object.freeze({
  schemaVersion: 35,
  baselineSchemaVersion: 34,
  replacedRules: {
    ui_typography_tiers: {
      current: ["title", "body", "auxiliary"],
      baseline: ["title", "secondary"],
    },
  },
  removedBaselineRules: {
    ui_secondary_non_title_font_size_unified_required: true,
    ui_third_font_size_tier_forbidden: true,
  },
  addedRules: {
    ui_body_font_size_unified_required: true,
    ui_body_typography_roles: ["body", "label", "button", "primary_status"],
    ui_auxiliary_font_size_unified_required: true,
    ui_auxiliary_typography_roles: ["helper", "metadata", "time", "secondary_status"],
    ui_fourth_font_size_tier_forbidden: true,
    green_storage_layout_required: true,
    green_storage_scope: "product_owned_files_and_runtime_payloads",
    immutable_application_payload_install_root_required: true,
    bundled_runtime_payload_install_root_required: true,
    bundled_runtime_copy_to_user_data_for_execution_forbidden: true,
    mutable_nonsecret_user_state_root: "%LOCALAPPDATA%\\LocalBridge",
    mutable_nonsecret_user_state_single_root_required: true,
    mutable_nonsecret_user_state_categories: ["settings", "workspace_registry", "task_state", "logs", "diagnostics"],
    unnecessary_program_data_footprint_forbidden: true,
    persistent_temp_product_payload_forbidden: true,
    product_owned_mutable_state_in_windows_system_directories_forbidden: true,
    runtime_api_key_windows_credential_manager_required: true,
    protected_per_machine_program_files_install_preserved: true,
    os_managed_installer_and_autostart_registration_allowed: true,
    ordinary_application_launch_token: "current_windows_user",
    ordinary_application_launch_integrity: "medium",
    foreground_ordinary_launch_uac_forbidden: true,
    background_ordinary_launch_uac_forbidden: true,
    login_autostart_ordinary_launch_uac_forbidden: true,
    only_privileged_broker_may_run_high_integrity: true,
  },
  lb015: {
    artifactReplacements: [[
      "three-tier typography system with exactly title, body and auxiliary font sizes and no fourth tier",
      "two-tier typography system with exactly title and unified secondary non-title font sizes",
    ]],
    testReplacements: [[
      "product typography uses exactly three font-size tiers: title for page/major headings, body for ordinary body labels buttons and primary status, and auxiliary for helper metadata time and secondary status; each non-title tier is internally unified and no fourth font-size tier exists",
      "all non-title text uses one unified secondary font-size token across body helper status metadata labels and button text; no third font-size tier exists",
    ]],
  },
  lb016: {
    testReplacements: [[
      "onboarding uses exactly three typography sizes: title for page/major headings, body for ordinary copy labels buttons and primary status, and auxiliary for helper metadata time and secondary status; no fourth font-size tier is introduced",
      "onboarding uses exactly two typography sizes: title and the same unified secondary size for every other text role; helper copy status labels button text and metadata cannot introduce a third font-size tier",
    ]],
  },
  lb018: {
    addedArtifacts: [
      "green-storage packaged layout with immutable application and bundled runtime payloads under the protected LocalBridge installation root, mutable non-secret per-user state under one %LOCALAPPDATA%\\LocalBridge root, and secrets remaining in Windows Credential Manager",
    ],
    addedTests: [
      "packaged immutable application and bundled runtime payloads including Python coding runtime Tunnel Broker and static resources execute from the canonical LocalBridge installation root; ordinary launch does not copy or extract those payloads into LocalAppData ProgramData Windows system directories or persistent Temp merely for execution",
      "mutable non-secret LocalBridge state including settings workspace registry task state logs and diagnostics persists under one %LOCALAPPDATA%\\LocalBridge root; product-owned persistent files do not create an unnecessary %ProgramData%\\LocalBridge footprint or mutable state in Windows system directories or persistent Temp",
      "Runtime API Key remains solely in Windows Credential Manager and is absent from install-root LocalAppData ProgramData and Temp plaintext files",
      "release-style foreground background and login-autostart ordinary LocalBridge launches use the current Windows user at Medium Integrity and do not trigger UAC; only explicit elevated_exec through Broker plus UAC may create High Integrity administrator execution while the main app and ordinary runtime routes remain Medium",
    ],
  },
  lb019: {
    addedTests: [
      "clean-machine footprint audit after install launch normal use and reboot finds product-owned persistent files only under the canonical installation root and the single %LOCALAPPDATA%\\LocalBridge mutable-state root, with Runtime API Key only in Windows Credential Manager; no unnecessary ProgramData Windows-system-directory or persistent-Temp runtime copy exists, excluding normal OS-managed installer shortcut and autostart registration metadata",
      "clean-machine foreground background and login-autostart launches complete without UAC and keep the LocalBridge application and ordinary runtime process tree at current-user Medium Integrity",
      "after explicit administrator-mode consent and UAC, the LocalBridge application and ordinary routes remain Medium Integrity while only the separate privileged Broker administrator route is High Integrity",
      "uninstall/orphan verification leaves no LocalBridge runtime executable or bundled-runtime copy in LocalAppData ProgramData Windows system directories or persistent Temp; any retained mutable user data stays confined to the documented single LocalAppData root",
    ],
  },
});

const ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15 = Object.freeze({
  schemaVersion: 33,
  baselineSchemaVersion: 32,
  replacedRules: {
    windows_system_management_programs: {
      baseline: ["reg.exe", "schtasks.exe", "sc.exe", "netsh.exe"],
      current: ["reg.exe", "schtasks.exe", "sc.exe", "netsh.exe", "bcdedit.exe", "dism.exe"],
    },
  },
  removedRules: {
    onboarding_screen_3_permission_min_height_px: 80,
    onboarding_two_line_button_min_rendered_height_multiplier: 2,
    reviewed_system_management_shell_interpreter_fallback_forbidden: true,
  },
  addedRules: {
    ui_text_button_content_sized_required: true,
    ui_text_button_width_basis: "maximum_visible_character_count_in_any_single_line",
    ui_text_button_height_basis: "rendered_line_count",
    ui_text_button_arbitrary_fixed_geometry_forbidden: true,
    ui_typography_tiers: ["title", "secondary"],
    ui_secondary_non_title_font_size_unified_required: true,
    ui_third_font_size_tier_forbidden: true,
    permission_mode_edit_workspace_bound: true,
    permission_mode_full_workspace_bound: true,
    permission_mode_elevated_workspace_bound: false,
    elevated_administrator_token_scope_required: true,
    elevated_full_filesystem_access_within_administrator_token: true,
    elevated_ordinary_and_administrator_process_command_execution_allowed: true,
    elevated_system_maintenance_allowed: true,
    elevated_admin_execution_requires_active_broker: true,
    elevated_privileged_filesystem_requires_broker_backend: true,
    elevated_admin_shell_uses_trusted_selector_required: true,
    elevated_arbitrary_shell_executable_path_forbidden: true,
    localbridge_control_plane_mutation_via_elevated_route_forbidden: true,
    dashboard_elevated_project_scope_label: "全目录访问",
    dashboard_elevated_project_scope_accent: "yellow",
    dashboard_elevated_project_switch_blocked: true,
    dashboard_elevated_project_switch_dialog: "管理员模式拥有系统管理员令牌范围内的文件访问能力，若要切换，请切换其他模式",
    admin_mode_user_consequence_acknowledgement_required: true,
  },
  lb006: {
    artifactReplacements: [
      ["mode-aware public path/workdir authority adapter: Edit and Full are active-workspace-relative while Elevated privileged operations may address administrator-token-scoped filesystem paths outside the active workspace", "active-workspace-relative public path/workdir validator with stable LocalBridge typed errors"],
      ["permission-scope-aware Git repository resolver: Edit and Full stop at the active workspace root while Elevated may resolve administrator-token-accessible repositories outside that root", "shared workspace-bounded nested Git repository resolver for status diff log show and blame"],
    ],
    testReplacements: [
      ["Edit and Full document image Git and exec workdir inputs remain active-workspace-relative and reject drive UNC verbatim POSIX absolute or parent traversal; Elevated privileged filesystem and administrator execution routes accept administrator-token-accessible absolute paths outside the active workspace without weakening Edit or Full", "document image Git and exec workdir workspace-bound public inputs accept active-workspace-relative paths and reject drive UNC verbatim POSIX absolute or parent traversal at the LocalBridge boundary without leaking upstream ABSOLUTE_PATH_DENIED"],
      ["Git repository discovery never escapes the active workspace in Edit or Full; Elevated may resolve repositories outside the active workspace only through its administrator-token scope, and git_diff never uses non-git fallback once the LocalBridge resolver has confirmed a repository", "Git repository discovery never escapes the active workspace and git_diff never uses non-git fallback once the LocalBridge resolver has confirmed a repository"],
    ],
    addedTests: ["Elevated path authority is mode-aware rather than a lexical workspace bypass: the ordinary user route stays workspace-bound, while operations outside the active workspace are dispatched only through a Broker-backed privileged filesystem or administrator execution route and are bounded by the administrator token"],
  },
  lb007: {
    testReplacements: [
      ["Full ordinary exec_command treats static Windows system-management targets reg.exe schtasks.exe sc.exe netsh.exe bcdedit.exe and dism.exe, including exact System32 paths and case-insensitive executable names, as requiring the privileged route; ordinary execution returns PrivilegedRouteNotAvailable and cannot cross the administrator boundary", "Full and Elevated ordinary exec_command treat static Windows system-management targets reg.exe schtasks.exe sc.exe and netsh.exe, including exact System32 paths and case-insensitive executable names, as requiring the privileged route; ordinary execution returns PrivilegedRouteNotAvailable and never executes them with a Broker token"],
      ["Elevated provides both an ordinary current-user route and a separate Active-Broker administrator route: ordinary exec_command never silently inherits the Broker token, while the administrator route may execute trusted system-management and general administrator commands without globally disabling those targets", "ordinary Elevated exec_command never inherits the Active Broker administrator token, and privilege classification of Windows system-management targets does not globally prohibit the same trusted System32 programs from the separate reviewed elevated_exec route"],
    ],
    addedTests: ["Edit and Full cannot obtain administrator-token filesystem process command or system-maintenance capability through workflow indirection; Elevated may declare those privileged capabilities only for the Broker-backed administrator route and LocalBridge control-plane remains deny-always"],
  },
  lb012: {
    artifactReplacements: [["general Broker-backed administrator execution and privileged filesystem route for Elevated mode", "reviewed Windows system-management elevated_exec profiles"]],
    addedArtifacts: ["Broker-backed privileged filesystem operations that are not constrained to the active workspace once Elevated is Active"],
    testReplacements: [
      ["Elevated plus Active Broker can execute administrator-token operations through trusted System32 reg.exe schtasks.exe sc.exe netsh.exe bcdedit.exe and dism.exe and can execute general administrator processes or commands within the administrator token scope", "Elevated plus Active Broker can execute reviewed structured operations through exact trusted System32 reg.exe schtasks.exe sc.exe and netsh.exe without globally disabling those Windows system-management programs"],
      ["Elevated administrator execution accepts structured direct program plus argv and a separate trusted logical shell selector for administrator command text; MCP cannot supply an arbitrary shell executable path", "reviewed Windows system-management elevated_exec accepts structured program and argv only and rejects cmd PowerShell or other shell/interpreter fallback"],
      ["Windows OS system management is not itself LocalBridge control-plane mutation, while attempts through any administrator route including reg schtasks sc netsh bcdedit dism or trusted administrator shells to mutate LocalBridge PermissionMode administrator consent UAC Broker activation WorkspaceRegistry credentials Tunnel MCP runtime PEP Broker policy or LocalBridge autostart remain denied", "Windows OS system management is not itself LocalBridge control-plane mutation, while attempts through reg.exe schtasks.exe sc.exe or netsh.exe to mutate LocalBridge PermissionMode administrator consent UAC Broker activation WorkspaceRegistry credentials Tunnel MCP runtime PEP Broker policy or LocalBridge autostart remain denied"],
    ],
    addedTests: [
      "Elevated plus Active Broker can read create modify rename and delete administrator-token-accessible filesystem objects outside the active workspace through a privileged filesystem route; Edit and Full remain unable to cross the active workspace authorization root",
      "Elevated plus Active Broker can execute a general administrator direct process and a trusted PowerShell or cmd logical selector outside the active workspace, with timeout cancellation output bounds and redaction preserved",
      "whole-app elevation remains forbidden: LocalBridge MCP Tunnel and ordinary exec stay under the normal user token and only Broker-dispatched administrator operations receive the administrator token",
    ],
  },
  lb015: {
    addedArtifacts: [
      "content-derived text-button geometry system whose width follows maximum visible characters per line and height follows rendered line count",
      "two-tier typography system with exactly title and unified secondary non-title font sizes",
      "Dashboard Elevated project-scope projection showing yellow 全目录访问 and blocking project switching with the fixed explanatory dialog",
    ],
    addedTests: [
      "every text-bearing button sizes from its own content metrics: width corresponds to the maximum visible character count on any single line and height corresponds to the actual rendered line count; arbitrary fixed button geometry that ignores text content is forbidden",
      "all non-title text uses one unified secondary font-size token across body helper status metadata labels and button text; no third font-size tier exists",
      "when PermissionMode is Elevated and Broker is Active the Dashboard current-project area displays yellow 全目录访问 instead of implying an active-workspace access boundary; clicking the project-switch affordance does not switch workspace and shows exactly 管理员模式拥有系统管理员令牌范围内的文件访问能力，若要切换，请切换其他模式",
    ],
  },
  lb016: {
    testReplacements: [
      ["screen 3 text-bearing permission buttons derive width from the maximum visible character count on any single line and height from the actual title plus description rendered line count; content must not be clipped and final visual PASS requires human inspection rather than CSS marker presence", "screen 3 permission mode buttons that contain title plus description render at least twice the actual rendered height of the ordinary single-line control at 780x620; both text line boxes are fully visible and final visual PASS requires human inspection rather than CSS marker presence"],
      ["screen 3 permission buttons pass a computed/rendered geometry Gate proving geometry follows content metrics rather than a fixed min-height or multiplier; title and description line boxes are complete and human 780x620 visual Gate remains required", "screen 3 two-line permission buttons pass a computed/rendered geometry Gate proving actual height at least 2x a single-line control and complete title/description line boxes; human 780x620 visual Gate remains required and static min-height CSS alone cannot PASS"],
    ],
    addedTests: ["onboarding uses exactly two typography sizes: title and the same unified secondary size for every other text role; helper copy status labels button text and metadata cannot introduce a third font-size tier"],
  },
});

const WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15 = Object.freeze({
  schemaVersion: 32,
  baselineSchemaVersion: 31,
  addedRules: {
    windows_system_management_programs: ["reg.exe", "schtasks.exe", "sc.exe", "netsh.exe"],
    ordinary_system_management_requires_privileged_route: true,
    elevated_ordinary_exec_never_inherits_broker_token: true,
    reviewed_system_management_elevated_exec_allowed: true,
    reviewed_system_management_system32_identity_required: true,
    reviewed_system_management_shell_interpreter_fallback_forbidden: true,
    localbridge_control_plane_mutation_via_system_management_forbidden: true,
  },
  lb007: {
    addedTests: [
      "Full and Elevated ordinary exec_command treat static Windows system-management targets reg.exe schtasks.exe sc.exe and netsh.exe, including exact System32 paths and case-insensitive executable names, as requiring the privileged route; ordinary execution returns PrivilegedRouteNotAvailable and never executes them with a Broker token",
      "ordinary Elevated exec_command never inherits the Active Broker administrator token, and privilege classification of Windows system-management targets does not globally prohibit the same trusted System32 programs from the separate reviewed elevated_exec route",
    ],
  },
  lb012: {
    addedArtifacts: [
      "reviewed Windows system-management elevated_exec profiles",
    ],
    addedTests: [
      "Edit and Full cannot use reviewed Windows system-management elevated_exec profiles and Elevated requires an Active Broker before any reviewed system-management execution",
      "Elevated plus Active Broker can execute reviewed structured operations through exact trusted System32 reg.exe schtasks.exe sc.exe and netsh.exe without globally disabling those Windows system-management programs",
      "reviewed Windows system-management elevated_exec rejects same-name PATH workspace or other non-System32 executables and requires the exact trusted System32 program identity",
      "reviewed Windows system-management elevated_exec accepts structured program and argv only and rejects cmd PowerShell or other shell/interpreter fallback",
      "Windows OS system management is not itself LocalBridge control-plane mutation, while attempts through reg.exe schtasks.exe sc.exe or netsh.exe to mutate LocalBridge PermissionMode administrator consent UAC Broker activation WorkspaceRegistry credentials Tunnel MCP runtime PEP Broker policy or LocalBridge autostart remain denied",
    ],
  },
});

const LB007_POLICY_AND_CMD_CODEPAGE_AMENDMENT_2026_08_15 = Object.freeze({
  schemaVersion: 31,
  baselineSchemaVersion: 30,
  addedRules: {
    powershell_standalone_literal_get_command_diagnostic_allowed_without_privilege_review: true,
    powershell_console_stdin_readtoend_narrow_safe_io_seam: true,
    powershell_dynamic_or_compound_command_resolution_remains_review_required: true,
    powershell_non_ascii_roundtrip_required: true,
    cmd_native_codepage_semantics_preserved: true,
    cmd_non_ascii_roundtrip_required: false,
    cmd_codepage_mutation_for_unicode_forbidden: true,
  },
  lb006: {
    addedTests: [
      "cmd selector preserves native cmd.exe code-page semantics: LocalBridge does not inject chcp or otherwise mutate the active code page solely to force Unicode, and exact Chinese or Emoji round-trip is not required when cmd's active code page cannot represent the characters; LocalBridge still returns a valid UTF-8 public string and must not add corruption beyond captured cmd output",
    ],
  },
  lb007: {
    addedTests: [
      "a standalone PowerShell Get-Command or gcm query with exactly one literal command-name argument is ordinary read-only command discovery in Full or Elevated and is not classified as privilege or review-required solely because it uses Get-Command; dynamic command names, additional pipeline or follow-on execution, ScriptBlock extraction, subexpressions, aliases functions provider mutation command-engine mutation or other runtime-selected command-resolution surfaces remain review-required and fail closed",
      "the narrow trusted PowerShell Console stdin seam permits [Console]::In.ReadLine() and [Console]::In.ReadToEnd() plus the existing Console output calls without treating those exact statically rooted calls as arbitrary instance-member dispatch; other instance-member calls remain review-required unless separately ratified",
    ],
  },
});

const NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15 = Object.freeze({
  schemaVersion: 30,
  baselineSchemaVersion: 29,
  addedRules: {
    agent_workflow_workspace_relative_project_path_selector_required: true,
    agent_workflow_nested_project_discovery_consistent_with_git_workflow_required: true,
    agent_workflow_project_selector_must_not_change_active_workspace_authority: true,
    trusted_powershell_standard_cmdlet_surface_required: true,
    trusted_powershell_arbitrary_module_autoload_forbidden: true,
    trusted_powershell_standard_module_preload_identity_validation_required: true,
    workspace_structured_directory_write_required: true,
    workspace_structured_directory_write_active_root_only: true,
    workspace_structured_directory_write_must_not_mutate_workspace_control_plane: true,
    workspace_structured_directory_reparse_escape_forbidden: true,
    powershell_provider_mutation_review_requirement_preserved: true,
    agent_workflow_structured_directory_changes_field_required: true,
    agent_workflow_directory_change_actions: ["create_directory", "remove_empty_directory"],
  },
  lb006: {
    addedArtifacts: [
      "agent_workflow workspace-relative nested-project selector with stable selected-path and enclosing-repository context",
      "trusted PowerShell standard cmdlet baseline loaded from validated first-party/system module identity before arbitrary module autoload is disabled",
      "agent_workflow directory_changes structured active-workspace directory create and empty-directory cleanup route without adding a ninth public core tool",
    ],
    addedTests: [
      "with active workspace D:\\project, agent_workflow path=LocalBridge resolves the explicit nested project D:\\project\\LocalBridge and git_before/git_after report the same repository identity as git_workflow path=LocalBridge; path=LocalBridge/src resolves the nearest enclosing LocalBridge repository without escaping the active workspace, while a non-repository directory may legitimately report is_repo=false",
      "agent_workflow project selection is workspace-relative and changes only workflow/project context: it never changes WorkspaceRegistry or the active workspace authorization root; the stable result exposes the selected path and enclosing repository/project context without raw private resolver state",
      "windows_powershell and auto resolving to PowerShell provide a trusted standard coding cmdlet baseline including Get-Location Get-ChildItem and Test-Path while keeping arbitrary module autoload disabled; the preload source is fixed or identity-validated and cannot be redirected through user-controlled PSModulePath",
      "restoring standard PowerShell cmdlets does not weaken shell review: provider mutation and dynamic command-surface operations such as New-Item Set-Content Set-Item Alias or Function provider mutation remain subject to the existing LB-007 review-required classification",
      "agent_workflow exposes optional directory_changes as a bounded array of objects with exactly action plus path; action is only create_directory or remove_empty_directory and path is active-workspace-relative; with active workspace D:\\project, directory_changes can create D:\\project\\test and later remove that directory when empty without using process execution; Edit and Full both permit this reviewed workspace write, absolute or parent-traversal targets and reparse escapes are denied, non-empty recursive directory deletion is not implied, and the operation never mutates WorkspaceRegistry or active workspace control-plane",
    ],
  },
  lb007: {
    addedArtifacts: [
      "structured agent_workflow directory_changes mutations classified as workspace write without granting control-plane authority",
    ],
    addedTests: [
      "agent_workflow structured directory create or empty-directory cleanup declares transitive workspace write capability: Edit and Full may authorize it inside the active root, process execution is not required, and attempts to alter WorkspaceRegistry active workspace or escape through absolute traversal or reparse targets fail closed",
    ],
  },
});

const COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15 = Object.freeze({
  schemaVersion: 29,
  baselineSchemaVersion: 28,
  addedRules: {
    command_terminal_unconditional_finalizer_required: true,
    command_terminal_finalizer_must_atomically_append_finished_and_clear_current: true,
    command_terminal_finished_event_exactly_once_required: true,
    task_state_terminal_snapshot_persistence_required: true,
    task_state_terminal_truth_independent_of_private_session_retention_required: true,
    task_state_command_owner_compare_and_swap_required: true,
    task_state_command_owner_identity: ["task_id", "session_id"],
    task_state_non_owner_overwrite_or_clear_forbidden: true,
    task_state_duplicate_terminal_finalization_idempotent: true,
    main_window_default_centered_required: true,
    existing_window_reopen_forced_recenter_forbidden: true,
  },
  lb006: {
    addedArtifacts: [
      "owner-checked atomic task-state command finalizer with durable terminal snapshots",
    ],
    addedTests: [
      "every command terminal path including success nonzero-exit failure timeout cancellation kill runtime error and tool exception executes one finally or finally-equivalent unconditional finalizer that under one task-state owner transaction appends exactly one command_finished terminal event and clears current_command only when the same task_id plus session_id still owns it",
      "task-state persists a bounded redacted terminal command snapshot sufficient to recover status outcome exit or signal timeout cancellation and stable output references after the private runtime session has expired or been pruned; the approximately 300-second private session retention may support output paging but is never the source of terminal truth",
      "task-state command start replace finish and clear operations use compare-and-swap or an equivalent owner check on task_id plus session_id; a delayed finalizer from task A cannot overwrite or clear task B or a newer session and an owner mismatch is a no-op or typed conflict rather than destructive mutation",
      "duplicate terminal callbacks for the same task_id plus session_id are idempotent: the terminal snapshot remains stable command_finished is not duplicated and current_command cannot be resurrected or clear a newer owner",
    ],
  },
  lb015: {
    addedArtifacts: [
      "default-centered first visible 780x620 main window creation",
    ],
    addedTests: [
      "on normal foreground first visible creation the fixed 780x620 main window is centered within the current monitor work area with DPI rounding tolerance; reopening an already-created hidden window preserves its user-moved position instead of forcibly recentering it",
    ],
  },
});

const TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14 = Object.freeze({
  schemaVersion: 28,
  baselineSchemaVersion: 27,
  addedRules: {
    test_orchestration_tiers: ["pr_fast", "pr_runtime", "group_release"],
    test_heavy_shared_fixture_lifecycle_compression_required: true,
    test_heavy_repeated_runtime_start_requires_isolation_rationale: true,
    test_cheap_unit_tests_forced_merge_forbidden: true,
    test_static_contract_behavioral_substitution_forbidden: true,
    test_duplicate_static_and_behavior_check_requires_distinct_contract_rationale: true,
    test_inner_loop_repeated_full_repo_gate_forbidden: true,
    test_command_session_lifecycle_real_e2e_required: true,
    test_command_bounded_timeout_cancel_required: true,
    test_lost_session_must_terminal_fail: true,
    development_console_window_presence_not_product_failure: true,
    development_console_window_free_required: false,
    packaged_gui_managed_child_visible_console_forbidden: true,
    packaged_console_requirement_must_be_release_style_verified: true,
    shell_command_exact_single_parse_semantics_required: true,
    command_poll_incremental_output_no_loss_no_replay_required: true,
    command_write_running_session_required: true,
    command_kill_cancel_terminal_convergence_required: true,
    command_kill_runtime_unavailable_on_healthy_runtime_forbidden: true,
    view_image_real_auto_resize_required: true,
    public_command_output_utf8_required: true,
    git_blame_line_range_one_based_inclusive: true,
    document_line_range_one_based_inclusive: true,
    invalid_line_range_typed_invalid_argument_required: true,
    runtime_result_semantic_probe_all_adapter_consumed_unmodeled_fields_required: true,
  },
  lb006: {
    addedArtifacts: [
      "schema28 public command lifecycle delta/cursor state for incremental poll and stable kill terminal convergence",
      "single-parse shell invocation and UTF-8 public output normalization across trusted Windows shell selectors",
      "real image resize adapter plus inclusive line-range validation for Git blame and documents",
    ],
    addedTests: [
      "LB-006 PR Fast Gate runs cheap deterministic unit fake and static contract checks without repeatedly invoking the complete repository Gate after each small edit",
      "LB-006 PR Runtime Gate groups heavy tests that share the bundled runtime PEP and process topology into a shared lifecycle unless isolation itself is under test; repeated runtime startup for independent assertions has an explicit isolation rationale and cheap isolated unit tests remain separate",
      "one real public command session lifecycle E2E uses a single bundled runtime and PEP lifecycle to cover exec then incremental poll then write then read then kill then terminal convergence with bounded timeout cancellation and explicit lost-session terminal handling",
      "static source-marker checks do not substitute for executable shell session image Git or document behavior and duplicate static plus behavior checks exist only for a distinct static contract",
      "PowerShell Write-Output \"a|b\" and Write-Output \"a&b\" plus their single-quoted equivalents preserve literal output with exactly one intended shell parse and no extra LocalBridge reparse",
      "a command emitting poll-1 poll-2 poll-3 over time exposes every chunk in order exactly once across command_control.poll calls and a later poll with no new output returns an empty delta plus state or terminal metadata rather than replaying poll-3",
      "a running public command session that waits for stdin accepts command_control.write after exec has returned and the written chars reach the process without SessionUnavailable",
      "command_control.kill on a valid running public session with a healthy runtime does not return RuntimeUnavailable and converges to a stable cancelled terminal snapshot that later poll calls keep instead of degrading to SessionUnavailable",
      "view_image auto_resize on a 1024x1024 source succeeds for maximum 512x512 and 64x64 with bounded dimensions and preserved aspect ratio while the no-resize path remains valid",
      "Windows PowerShell Write-Output 中文输出测试 round-trips exact UTF-8 under windows_powershell and under auto when auto resolves to PowerShell without mojibake or replacement characters",
      "git_workflow blame uses one-based inclusive start_line and end_line so 5 through 5 returns exactly line 5 and 1 through 3 returns exactly three lines; start_line greater than end_line is InvalidArgument and max_lines remains bounded",
      "document_workflow line ranges are one-based inclusive and start_line greater than end_line returns LocalBridge InvalidArgument rather than a successful empty result",
      "every adapter-consumed private result field not guaranteed by upstream outputSchema has a deterministic non-destructive semantic compatibility probe or equivalent fail-closed evidence before public facade serving",
    ],
  },
  lb018: {
    addedTests: [
      "release-style packaged GUI launch proves LocalBridge-owned managed background children do not create unintended visible console windows while preserving Job/process ownership; development test-harness console windows are not used as evidence for this packaged behavior",
    ],
  },
  lb019: {
    addedTests: [
      "clean-machine packaged GUI execution shows no unintended visible console windows from LocalBridge-owned bundled runtime Tunnel Broker/background helpers or managed shell/direct command children unless an explicit future interactive-terminal contract opts in",
    ],
  },
});

const PUBLIC_FACADE_RUNTIME_SEMANTICS_AMENDMENT_2026_08_14 = Object.freeze({
  schemaVersion: 27,
  baselineSchemaVersion: 26,
  addedRules: {
    public_session_ids_localbridge_owned: true,
    public_output_refs_localbridge_owned: true,
    upstream_private_session_handles_public_forbidden: true,
    upstream_private_output_handles_public_forbidden: true,
    command_control_poll_live_session_by_public_session_id_required: true,
    command_control_poll_retained_output_mapping_forbidden: true,
    command_control_read_uses_public_output_ref: true,
    public_session_terminal_convergence_without_client_poll_required: true,
    public_session_permanent_running_after_private_prune_forbidden: true,
    public_session_lost_error_code: "SessionUnavailable",
    nonzero_process_exit_public_success_forbidden: true,
    nonzero_process_exit_error_code: "ProcessFailed",
    workspace_context_workspace_non_empty_ordinary_absolute_required: true,
    workspace_context_default_cwd_workspace_relative_required: true,
    workspace_bound_public_path_inputs_relative_only: true,
    workspace_public_absolute_path_input_forbidden: true,
    workspace_public_parent_traversal_input_forbidden: true,
    runtime_result_semantic_probe_required_when_output_schema_insufficient: true,
    public_facade_advertised_action_must_be_implemented: true,
    agent_workflow_actions: ["diagnose", "bugfix", "feature", "refactor", "test_failure", "build_release", "document", "resume", "custom"],
    task_control_actions: ["get", "cancel"],
    document_workflow_actions: ["inspect", "create", "convert", "rebuild"],
    git_nested_repository_resolution_consistent: true,
    git_repository_discovery_stops_at_active_workspace_root: true,
    git_directory_path_selects_repo_context: true,
    git_paths_are_path_filters: true,
    git_diff_non_git_fallback_after_repo_discovery_forbidden: true,
  },
  lb006: {
    addedArtifacts: [
      "LocalBridge-owned public command Session Manager with opaque public session/output handles and terminal snapshots",
      "stable command_control live-session versus retained-output adapter for poll/read/write/kill",
      "workspace_context stable projection plus deterministic private-result semantic compatibility probe",
      "stable process outcome normalizer that maps every nonzero ordinary exit to ProcessFailed",
      "complete executable implementations for every advertised v1 agent_workflow task_control and document_workflow action",
      "active-workspace-relative public path/workdir validator with stable LocalBridge typed errors",
      "shared workspace-bounded nested Git repository resolver for status diff log show and blame",
    ],
    addedTests: [
      "active workspace D:\\project yields non-empty ordinary workspace_context.workspace D:\\project plus workspace-relative default_cwd and never a Win32 verbatim projection",
      "if private get_default_cwd result semantics do not provide non-empty workspace:string plus default_cwd:string the facade fails closed with RuntimeCapabilityMismatch before serving public tools",
      "a long-running exec returns LocalBridge-owned public session/output handles; command_control poll write and kill use the public session handle while retained output read uses the public output handle",
      "public session_id and output_ref do not equal or expose upstream private session/output handles",
      "a running public session converges to completed failed timed_out cancelled or lost without requiring continued client poll; private pruning runtime restart or handle loss cannot leave permanent Running and unobserved terminal loss returns SessionUnavailable",
      "cmd /c exit 7 with empty stdout and stderr returns public ProcessFailed with ok=false isError=true and Failed CurrentTask; exit_code zero remains completed success",
      "every advertised agent_workflow task_control and document_workflow action executes its stable contract and none returns a permanent currently-unavailable response",
      "document image Git and exec workdir workspace-bound public inputs accept active-workspace-relative paths and reject drive UNC verbatim POSIX absolute or parent traversal at the LocalBridge boundary without leaking upstream ABSOLUTE_PATH_DENIED",
      "with active workspace D:\\project and nested repo D:\\project\\LocalBridge, git status log show and diff resolve the same repository and blame of LocalBridge/package.json resolves its enclosing repository",
      "Git repository discovery never escapes the active workspace and git_diff never uses non-git fallback once the LocalBridge resolver has confirmed a repository",
    ],
  },
});

const G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14 = Object.freeze({
  schemaVersion: 24,
  baselineCommit: "369b84d59d98a2ce2d2aa06ccd27d7b403a27806",
  baselineSchemaVersion: 23,
  addedRules: {
    scrollbar_arrow_affordances_forbidden: true,
    foreground_ui_ready_before_runtime_start_required: true,
    foreground_service_start_before_ui_ready_forbidden: true,
    foreground_ui_ready_intent_typed_one_shot: true,
    foreground_ui_ready_backend_idempotent: true,
    foreground_ui_ready_frontend_lifecycle_ownership_forbidden: true,
    background_launch_must_not_wait_for_ui_ready: true,
    existing_background_runtime_wake_must_not_restart_for_ui_ready: true,
  },
  lb014: {
    artifactReplacements: [[
      "UI-first configured foreground launch automatic runtime start gated by typed UI-ready",
      "foreground configured launch automatic runtime start",
    ]],
    testReplacements: [
      [
        "configured foreground UI launch automatically starts selected project runtime MCP and OpenAI Tunnel after the interactive UI emits one typed UI-ready intent and without a second user service-start action",
        "configured foreground UI launch automatically starts selected project runtime MCP and OpenAI Tunnel without a second user start action",
      ],
      [
        "foreground UI is created shown and interactive before backend managed-service startup begins, then projects Starting Ready or Fault while backend startup runs",
        "foreground window remains responsive and projects Starting Ready or Fault while backend startup runs",
      ],
    ],
    addedTests: [
      "before typed UI-ready, a configured foreground launch with stopped runtime does not start selected project runtime MCP or OpenAI Tunnel",
      "duplicate UI-ready delivery is backend-idempotent and cannot create a second managed runtime owner",
      "--background startup does not wait for UI-ready and waking an existing healthy background runtime does not restart it merely to replay the foreground UI-ready gate",
    ],
  },
  lb015: {
    addedArtifacts: ["arrowless rounded-scroll presentation"],
    addedTests: [
      "vertical scrollbars on Settings and other scrollable rounded surfaces expose no top or bottom arrow button or triangle affordance at 780x620",
      "removing scrollbar arrow affordances preserves wheel track and thumb scrolling and does not flatten or cut any outer rounded corner",
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

export function hasExactG3ManualReviewRound2_20260814(contractsDoc) {
  if (contractsDoc?.schema_version !== G3_MANUAL_REVIEW_ROUND2_2026_08_14.schemaVersion) return false;
  const rules = contractsDoc?.rules;
  for (const [key, [current]] of Object.entries(G3_MANUAL_REVIEW_ROUND2_2026_08_14.replacedRules)) {
    if (JSON.stringify(rules?.[key]) !== JSON.stringify(current)) return false;
  }
  for (const [key, expected] of Object.entries(G3_MANUAL_REVIEW_ROUND2_2026_08_14.addedRules)) {
    if (JSON.stringify(rules?.[key]) !== JSON.stringify(expected)) return false;
  }

  const lb015 = contractsDoc?.prs?.["LB-015"];
  const lb016 = contractsDoc?.prs?.["LB-016"];
  if (!lb015 || !lb016) return false;
  if (!containsAll(lb015.writable_paths, G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb015.addedWritablePaths)) return false;
  if (!containsAll(lb015.required_artifacts, G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb015.addedArtifacts)) return false;
  if (!containsAll(lb015.required_tests, G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb015.addedTests)) return false;
  for (const [current, old] of G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb015.artifactReplacements) {
    if (!lb015.required_artifacts?.includes(current) || lb015.required_artifacts?.includes(old)) return false;
  }
  for (const [current, old] of G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb015.testReplacements) {
    if (!lb015.required_tests?.includes(current) || lb015.required_tests?.includes(old)) return false;
  }
  for (const [current, old] of G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb016.artifactReplacements) {
    if (!lb016.required_artifacts?.includes(current) || lb016.required_artifacts?.includes(old)) return false;
  }
  if (!containsAll(lb016.required_tests, G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb016.addedTests)) return false;
  for (const [current, old] of G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb016.testReplacements) {
    if (!lb016.required_tests?.includes(current) || lb016.required_tests?.includes(old)) return false;
  }
  return true;
}

export function normalizeG3ManualReviewRound2_20260814(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = G3_MANUAL_REVIEW_ROUND2_2026_08_14.baselineSchemaVersion;
  for (const [key, [, old]] of Object.entries(G3_MANUAL_REVIEW_ROUND2_2026_08_14.replacedRules)) normalized.rules[key] = old;
  for (const key of Object.keys(G3_MANUAL_REVIEW_ROUND2_2026_08_14.addedRules)) delete normalized.rules[key];

  const lb015 = normalized.prs?.["LB-015"];
  if (lb015) {
    lb015.writable_paths = removeItems(lb015.writable_paths, G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb015.addedWritablePaths);
    lb015.required_artifacts = removeItems(lb015.required_artifacts, G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb015.addedArtifacts)
      .map((item) => G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb015.artifactReplacements.find(([current]) => current === item)?.[1] ?? item);
    lb015.required_tests = removeItems(lb015.required_tests, G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb015.addedTests)
      .map((item) => G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb015.testReplacements.find(([current]) => current === item)?.[1] ?? item);
  }

  const lb016 = normalized.prs?.["LB-016"];
  if (lb016) {
    lb016.required_artifacts = lb016.required_artifacts
      .map((item) => G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb016.artifactReplacements.find(([current]) => current === item)?.[1] ?? item);
    lb016.required_tests = removeItems(lb016.required_tests, G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb016.addedTests)
      .map((item) => G3_MANUAL_REVIEW_ROUND2_2026_08_14.lb016.testReplacements.find(([current]) => current === item)?.[1] ?? item);
  }
  return normalized;
}

export function hasExactAdminModeSafetyWarningAmendment20260814(contractsDoc) {
  if (contractsDoc?.schema_version !== ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.schemaVersion) return false;
  const rules = contractsDoc?.rules;
  for (const key of Object.keys(ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.removedBaselineRules)) {
    if (Object.hasOwn(rules ?? {}, key)) return false;
  }
  for (const [key, [current]] of Object.entries(ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.replacedRules)) {
    if (canonicalJson(rules?.[key]) !== canonicalJson(current)) return false;
  }
  for (const [key, expected] of Object.entries(ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.addedRules)) {
    if (canonicalJson(rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb015 = contractsDoc?.prs?.["LB-015"];
  const lb016 = contractsDoc?.prs?.["LB-016"];
  if (!lb015 || !lb016) return false;
  for (const [current, old] of ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb015.artifactReplacements) {
    if (!lb015.required_artifacts?.includes(current) || lb015.required_artifacts.includes(old)) return false;
  }
  for (const [current, old] of ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb015.testReplacements) {
    if (!lb015.required_tests?.includes(current) || lb015.required_tests.includes(old)) return false;
  }
  if (!containsAll(lb015.required_tests, ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb015.addedTests)) return false;
  for (const [current, old] of ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb016.artifactReplacements) {
    if (!lb016.required_artifacts?.includes(current) || lb016.required_artifacts.includes(old)) return false;
  }
  for (const [current, old] of ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb016.testReplacements) {
    if (!lb016.required_tests?.includes(current) || lb016.required_tests.includes(old)) return false;
  }
  return containsAll(lb016.required_tests, ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb016.addedTests);
}

export function normalizeAdminModeSafetyWarningAmendment20260814(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.baselineSchemaVersion;
  for (const [key, [, old]] of Object.entries(ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.replacedRules)) normalized.rules[key] = old;
  for (const [key, old] of Object.entries(ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.removedBaselineRules)) normalized.rules[key] = old;
  for (const key of Object.keys(ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.addedRules)) delete normalized.rules[key];
  const lb015 = normalized.prs?.["LB-015"];
  const lb016 = normalized.prs?.["LB-016"];
  if (lb015) {
    lb015.required_artifacts = lb015.required_artifacts.map((item) => ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb015.artifactReplacements.find(([current]) => current === item)?.[1] ?? item);
    lb015.required_tests = removeItems(lb015.required_tests, ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb015.addedTests)
      .map((item) => ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb015.testReplacements.find(([current]) => current === item)?.[1] ?? item);
  }
  if (lb016) {
    lb016.required_artifacts = lb016.required_artifacts.map((item) => ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb016.artifactReplacements.find(([current]) => current === item)?.[1] ?? item);
    lb016.required_tests = removeItems(lb016.required_tests, ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb016.addedTests)
      .map((item) => ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.lb016.testReplacements.find(([current]) => current === item)?.[1] ?? item);
  }
  return normalized;
}

export function hasExactCommandTaskStateAndWindowCenterAmendment20260815(contractsDoc) {
  if (contractsDoc?.schema_version !== COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.schemaVersion) return false;
  for (const [key, expected] of Object.entries(COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb006 = contractsDoc?.prs?.["LB-006"];
  const lb015 = contractsDoc?.prs?.["LB-015"];
  if (!lb006 || !lb015) return false;
  return containsAll(lb006.required_artifacts, COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.lb006.addedArtifacts)
    && containsAll(lb006.required_tests, COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.lb006.addedTests)
    && containsAll(lb015.required_artifacts, COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.lb015.addedArtifacts)
    && containsAll(lb015.required_tests, COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.lb015.addedTests);
}

export function normalizeCommandTaskStateAndWindowCenterAmendment20260815(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.baselineSchemaVersion;
  for (const key of Object.keys(COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.addedRules)) delete normalized.rules[key];
  const lb006 = normalized.prs?.["LB-006"];
  const lb015 = normalized.prs?.["LB-015"];
  if (lb006) {
    lb006.required_artifacts = removeItems(lb006.required_artifacts, COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.lb006.addedArtifacts);
    lb006.required_tests = removeItems(lb006.required_tests, COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.lb006.addedTests);
  }
  if (lb015) {
    lb015.required_artifacts = removeItems(lb015.required_artifacts, COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.lb015.addedArtifacts);
    lb015.required_tests = removeItems(lb015.required_tests, COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.lb015.addedTests);
  }
  return normalized;
}

export function hasExactLb007PolicyAndCmdCodepageAmendment20260815(contractsDoc) {
  if (contractsDoc?.schema_version !== LB007_POLICY_AND_CMD_CODEPAGE_AMENDMENT_2026_08_15.schemaVersion) return false;
  for (const [key, expected] of Object.entries(LB007_POLICY_AND_CMD_CODEPAGE_AMENDMENT_2026_08_15.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb006 = contractsDoc?.prs?.["LB-006"];
  const lb007 = contractsDoc?.prs?.["LB-007"];
  return Boolean(lb006 && lb007)
    && containsAll(lb006.required_tests, LB007_POLICY_AND_CMD_CODEPAGE_AMENDMENT_2026_08_15.lb006.addedTests)
    && containsAll(lb007.required_tests, LB007_POLICY_AND_CMD_CODEPAGE_AMENDMENT_2026_08_15.lb007.addedTests);
}

function hasReplacement(array, replacements) {
  return replacements.every(([current, baseline]) => array?.includes(current) && !array?.includes(baseline));
}

function normalizeReplacements(array, replacements) {
  return (array ?? []).map((item) => {
    const replacement = replacements.find(([current]) => current === item);
    return replacement ? replacement[1] : item;
  });
}

export function hasExactPublicMcpOutputSchemaAmendment20260816(contractsDoc) {
  if (contractsDoc?.schema_version !== PUBLIC_MCP_OUTPUT_SCHEMA_AMENDMENT_2026_08_16.schemaVersion) return false;
  const lb006 = contractsDoc?.prs?.["LB-006"];
  return Boolean(lb006)
    && containsAll(lb006.required_artifacts, PUBLIC_MCP_OUTPUT_SCHEMA_AMENDMENT_2026_08_16.lb006.addedArtifacts)
    && containsAll(lb006.required_tests, PUBLIC_MCP_OUTPUT_SCHEMA_AMENDMENT_2026_08_16.lb006.addedTests);
}

export function normalizePublicMcpOutputSchemaAmendment20260816(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = PUBLIC_MCP_OUTPUT_SCHEMA_AMENDMENT_2026_08_16.baselineSchemaVersion;
  const lb006 = normalized.prs?.["LB-006"];
  if (lb006) {
    lb006.required_artifacts = removeItems(lb006.required_artifacts, PUBLIC_MCP_OUTPUT_SCHEMA_AMENDMENT_2026_08_16.lb006.addedArtifacts);
    lb006.required_tests = removeItems(lb006.required_tests, PUBLIC_MCP_OUTPUT_SCHEMA_AMENDMENT_2026_08_16.lb006.addedTests);
  }
  return normalized;
}

export function hasExactG3UiTrayRefinementAmendment20260816(contractsDoc) {
  if (contractsDoc?.schema_version !== G3_UI_TRAY_REFINEMENT_AMENDMENT_2026_08_16.schemaVersion) return false;
  for (const [key, replacement] of Object.entries(G3_UI_TRAY_REFINEMENT_AMENDMENT_2026_08_16.replacedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(replacement.current)) return false;
  }
  for (const [key, expected] of Object.entries(G3_UI_TRAY_REFINEMENT_AMENDMENT_2026_08_16.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  for (const [id, delta] of Object.entries(G3_UI_TRAY_REFINEMENT_AMENDMENT_2026_08_16.prs)) {
    const pr = contractsDoc?.prs?.[id];
    if (!pr) return false;
    if (!containsAll(pr.writable_paths ?? [], delta.addedWritablePaths ?? [])) return false;
    if (!hasReplacement(pr.required_artifacts, delta.artifactReplacements ?? [])) return false;
    if (!hasReplacement(pr.required_tests, delta.testReplacements ?? [])) return false;
    if (!containsAll(pr.required_tests ?? [], delta.addedTests ?? [])) return false;
  }
  return true;
}

export function normalizeG3UiTrayRefinementAmendment20260816(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = G3_UI_TRAY_REFINEMENT_AMENDMENT_2026_08_16.baselineSchemaVersion;
  for (const [key, replacement] of Object.entries(G3_UI_TRAY_REFINEMENT_AMENDMENT_2026_08_16.replacedRules)) normalized.rules[key] = replacement.baseline;
  for (const key of Object.keys(G3_UI_TRAY_REFINEMENT_AMENDMENT_2026_08_16.addedRules)) delete normalized.rules[key];
  for (const [id, delta] of Object.entries(G3_UI_TRAY_REFINEMENT_AMENDMENT_2026_08_16.prs)) {
    const pr = normalized.prs?.[id];
    if (!pr) continue;
    if (Array.isArray(pr.writable_paths)) pr.writable_paths = removeItems(pr.writable_paths, delta.addedWritablePaths);
    if (Array.isArray(pr.required_artifacts)) pr.required_artifacts = normalizeReplacements(pr.required_artifacts, delta.artifactReplacements ?? []);
    if (Array.isArray(pr.required_tests)) {
      pr.required_tests = removeItems(pr.required_tests, delta.addedTests);
      pr.required_tests = normalizeReplacements(pr.required_tests, delta.testReplacements ?? []);
    }
  }
  return normalized;
}

export function hasExactPermissionModeEqualThirdsGeometryAmendment20260816(contractsDoc) {
  if (contractsDoc?.schema_version !== PERMISSION_MODE_EQUAL_THIRDS_GEOMETRY_AMENDMENT_2026_08_16.schemaVersion) return false;
  for (const [key, expected] of Object.entries(PERMISSION_MODE_EQUAL_THIRDS_GEOMETRY_AMENDMENT_2026_08_16.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb015 = contractsDoc?.prs?.["LB-015"];
  const lb016 = contractsDoc?.prs?.["LB-016"];
  if (!lb015 || !lb016) return false;
  return hasReplacement(lb015.required_tests, PERMISSION_MODE_EQUAL_THIRDS_GEOMETRY_AMENDMENT_2026_08_16.lb015.testReplacements)
    && hasReplacement(lb016.required_tests, PERMISSION_MODE_EQUAL_THIRDS_GEOMETRY_AMENDMENT_2026_08_16.lb016.testReplacements);
}

export function normalizePermissionModeEqualThirdsGeometryAmendment20260816(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = PERMISSION_MODE_EQUAL_THIRDS_GEOMETRY_AMENDMENT_2026_08_16.baselineSchemaVersion;
  for (const key of Object.keys(PERMISSION_MODE_EQUAL_THIRDS_GEOMETRY_AMENDMENT_2026_08_16.addedRules)) delete normalized.rules[key];
  const lb015 = normalized.prs?.["LB-015"];
  const lb016 = normalized.prs?.["LB-016"];
  if (lb015) lb015.required_tests = normalizeReplacements(lb015.required_tests, PERMISSION_MODE_EQUAL_THIRDS_GEOMETRY_AMENDMENT_2026_08_16.lb015.testReplacements);
  if (lb016) lb016.required_tests = normalizeReplacements(lb016.required_tests, PERMISSION_MODE_EQUAL_THIRDS_GEOMETRY_AMENDMENT_2026_08_16.lb016.testReplacements);
  return normalized;
}

export function hasExactUiGreenStorageDefaultNonAdminAmendment20260816(contractsDoc) {
  if (contractsDoc?.schema_version !== UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.schemaVersion) return false;
  for (const [key, replacement] of Object.entries(UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.replacedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(replacement.current)) return false;
  }
  for (const key of Object.keys(UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.removedBaselineRules)) {
    if (Object.hasOwn(contractsDoc?.rules ?? {}, key)) return false;
  }
  for (const [key, expected] of Object.entries(UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb015 = contractsDoc?.prs?.["LB-015"];
  const lb016 = contractsDoc?.prs?.["LB-016"];
  const lb018 = contractsDoc?.prs?.["LB-018"];
  const lb019 = contractsDoc?.prs?.["LB-019"];
  if (!lb015 || !lb016 || !lb018 || !lb019) return false;
  return hasReplacement(lb015.required_artifacts, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb015.artifactReplacements)
    && hasReplacement(lb015.required_tests, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb015.testReplacements)
    && hasReplacement(lb016.required_tests, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb016.testReplacements)
    && containsAll(lb018.required_artifacts, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb018.addedArtifacts)
    && containsAll(lb018.required_tests, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb018.addedTests)
    && containsAll(lb019.required_tests, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb019.addedTests);
}

export function normalizeUiGreenStorageDefaultNonAdminAmendment20260816(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.baselineSchemaVersion;
  for (const [key, replacement] of Object.entries(UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.replacedRules)) normalized.rules[key] = structuredClone(replacement.baseline);
  for (const [key, value] of Object.entries(UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.removedBaselineRules)) normalized.rules[key] = structuredClone(value);
  for (const key of Object.keys(UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.addedRules)) delete normalized.rules[key];
  const lb015 = normalized.prs?.["LB-015"];
  const lb016 = normalized.prs?.["LB-016"];
  const lb018 = normalized.prs?.["LB-018"];
  const lb019 = normalized.prs?.["LB-019"];
  if (lb015) {
    lb015.required_artifacts = normalizeReplacements(lb015.required_artifacts, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb015.artifactReplacements);
    lb015.required_tests = normalizeReplacements(lb015.required_tests, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb015.testReplacements);
  }
  if (lb016) lb016.required_tests = normalizeReplacements(lb016.required_tests, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb016.testReplacements);
  if (lb018) {
    lb018.required_artifacts = removeItems(lb018.required_artifacts, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb018.addedArtifacts);
    lb018.required_tests = removeItems(lb018.required_tests, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb018.addedTests);
  }
  if (lb019) lb019.required_tests = removeItems(lb019.required_tests, UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.lb019.addedTests);
  return normalized;
}

export function hasExactPermissionExecutionModelAmendment20260816(contractsDoc) {
  if (contractsDoc?.schema_version !== PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.schemaVersion) return false;
  for (const key of Object.keys(PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.removedRules)) {
    if (Object.hasOwn(contractsDoc?.rules ?? {}, key)) return false;
  }
  for (const [key, expected] of Object.entries(PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb006 = contractsDoc?.prs?.["LB-006"];
  const lb007 = contractsDoc?.prs?.["LB-007"];
  const lb012 = contractsDoc?.prs?.["LB-012"];
  if (!lb006 || !lb007 || !lb012) return false;
  return hasReplacement(lb006.required_artifacts, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb006.artifactReplacements)
    && hasReplacement(lb006.required_tests, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb006.testReplacements)
    && containsAll(lb006.required_tests, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb006.addedTests)
    && containsAll(lb007.required_artifacts, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb007.addedArtifacts)
    && hasReplacement(lb007.required_tests, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb007.testReplacements)
    && containsAll(lb007.required_tests, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb007.addedTests)
    && containsAll(lb012.required_artifacts, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb012.addedArtifacts)
    && hasReplacement(lb012.required_tests, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb012.testReplacements)
    && containsAll(lb012.required_tests, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb012.addedTests);
}

export function normalizePermissionExecutionModelAmendment20260816(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.baselineSchemaVersion;
  for (const key of Object.keys(PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.addedRules)) delete normalized.rules[key];
  for (const [key, old] of Object.entries(PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.removedRules)) normalized.rules[key] = structuredClone(old);
  const lb006 = normalized.prs?.["LB-006"];
  const lb007 = normalized.prs?.["LB-007"];
  const lb012 = normalized.prs?.["LB-012"];
  if (lb006) {
    lb006.required_artifacts = normalizeReplacements(lb006.required_artifacts, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb006.artifactReplacements);
    lb006.required_tests = removeItems(normalizeReplacements(lb006.required_tests, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb006.testReplacements), PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb006.addedTests);
  }
  if (lb007) {
    lb007.required_artifacts = removeItems(lb007.required_artifacts, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb007.addedArtifacts);
    lb007.required_tests = removeItems(normalizeReplacements(lb007.required_tests, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb007.testReplacements), PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb007.addedTests);
  }
  if (lb012) {
    lb012.required_artifacts = removeItems(lb012.required_artifacts, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb012.addedArtifacts);
    lb012.required_tests = removeItems(normalizeReplacements(lb012.required_tests, PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb012.testReplacements), PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.lb012.addedTests);
  }
  return normalized;
}

export function hasExactElevatedScopeAndUiGeometryAmendment20260815(contractsDoc) {
  if (contractsDoc?.schema_version !== ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.schemaVersion) return false;
  for (const [key, expected] of Object.entries(ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  for (const [key, replacement] of Object.entries(ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.replacedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(replacement.current)) return false;
  }
  for (const key of Object.keys(ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.removedRules)) {
    if (Object.hasOwn(contractsDoc?.rules ?? {}, key)) return false;
  }
  const lb006=contractsDoc?.prs?.["LB-006"], lb007=contractsDoc?.prs?.["LB-007"], lb012=contractsDoc?.prs?.["LB-012"], lb015=contractsDoc?.prs?.["LB-015"], lb016=contractsDoc?.prs?.["LB-016"];
  if (!lb006 || !lb007 || !lb012 || !lb015 || !lb016) return false;
  return hasReplacement(lb006.required_artifacts, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb006.artifactReplacements)
    && hasReplacement(lb006.required_tests, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb006.testReplacements)
    && containsAll(lb006.required_tests, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb006.addedTests)
    && hasReplacement(lb007.required_tests, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb007.testReplacements)
    && containsAll(lb007.required_tests, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb007.addedTests)
    && hasReplacement(lb012.required_artifacts, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb012.artifactReplacements)
    && containsAll(lb012.required_artifacts, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb012.addedArtifacts)
    && hasReplacement(lb012.required_tests, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb012.testReplacements)
    && containsAll(lb012.required_tests, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb012.addedTests)
    && containsAll(lb015.required_artifacts, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb015.addedArtifacts)
    && containsAll(lb015.required_tests, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb015.addedTests)
    && hasReplacement(lb016.required_tests, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb016.testReplacements)
    && containsAll(lb016.required_tests, ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb016.addedTests);
}

export function normalizeElevatedScopeAndUiGeometryAmendment20260815(contractsDoc) {
  const normalized=structuredClone(contractsDoc ?? null); if(!normalized) return normalized;
  normalized.schema_version=ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.baselineSchemaVersion;
  for (const key of Object.keys(ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.addedRules)) delete normalized.rules[key];
  for (const [key, replacement] of Object.entries(ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.replacedRules)) normalized.rules[key]=structuredClone(replacement.baseline);
  for (const [key, value] of Object.entries(ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.removedRules)) normalized.rules[key]=structuredClone(value);
  const lb006=normalized.prs?.["LB-006"], lb007=normalized.prs?.["LB-007"], lb012=normalized.prs?.["LB-012"], lb015=normalized.prs?.["LB-015"], lb016=normalized.prs?.["LB-016"];
  if(lb006){lb006.required_artifacts=normalizeReplacements(lb006.required_artifacts,ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb006.artifactReplacements);lb006.required_tests=removeItems(normalizeReplacements(lb006.required_tests,ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb006.testReplacements),ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb006.addedTests);}
  if(lb007) lb007.required_tests=removeItems(normalizeReplacements(lb007.required_tests,ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb007.testReplacements),ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb007.addedTests);
  if(lb012){lb012.required_artifacts=removeItems(normalizeReplacements(lb012.required_artifacts,ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb012.artifactReplacements),ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb012.addedArtifacts);lb012.required_tests=removeItems(normalizeReplacements(lb012.required_tests,ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb012.testReplacements),ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb012.addedTests);}
  if(lb015){lb015.required_artifacts=removeItems(lb015.required_artifacts,ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb015.addedArtifacts);lb015.required_tests=removeItems(lb015.required_tests,ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb015.addedTests);}
  if(lb016) lb016.required_tests=removeItems(normalizeReplacements(lb016.required_tests,ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb016.testReplacements),ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.lb016.addedTests);
  return normalized;
}

export function hasExactWindowsSystemManagementPrivilegeAmendment20260815(contractsDoc) {
  if (contractsDoc?.schema_version !== WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.schemaVersion) return false;
  for (const [key, expected] of Object.entries(WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb007 = contractsDoc?.prs?.["LB-007"];
  const lb012 = contractsDoc?.prs?.["LB-012"];
  return Boolean(lb007 && lb012)
    && containsAll(lb007.required_tests, WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.lb007.addedTests)
    && containsAll(lb012.required_artifacts, WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.lb012.addedArtifacts)
    && containsAll(lb012.required_tests, WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.lb012.addedTests);
}

export function normalizeWindowsSystemManagementPrivilegeAmendment20260815(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.baselineSchemaVersion;
  for (const key of Object.keys(WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.addedRules)) delete normalized.rules[key];
  const lb007 = normalized.prs?.["LB-007"];
  const lb012 = normalized.prs?.["LB-012"];
  if (lb007) lb007.required_tests = removeItems(lb007.required_tests, WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.lb007.addedTests);
  if (lb012) {
    lb012.required_artifacts = removeItems(lb012.required_artifacts, WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.lb012.addedArtifacts);
    lb012.required_tests = removeItems(lb012.required_tests, WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.lb012.addedTests);
  }
  return normalized;
}

export function normalizeLb007PolicyAndCmdCodepageAmendment20260815(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = LB007_POLICY_AND_CMD_CODEPAGE_AMENDMENT_2026_08_15.baselineSchemaVersion;
  for (const key of Object.keys(LB007_POLICY_AND_CMD_CODEPAGE_AMENDMENT_2026_08_15.addedRules)) delete normalized.rules[key];
  const lb006 = normalized.prs?.["LB-006"];
  const lb007 = normalized.prs?.["LB-007"];
  if (lb006) lb006.required_tests = removeItems(lb006.required_tests, LB007_POLICY_AND_CMD_CODEPAGE_AMENDMENT_2026_08_15.lb006.addedTests);
  if (lb007) lb007.required_tests = removeItems(lb007.required_tests, LB007_POLICY_AND_CMD_CODEPAGE_AMENDMENT_2026_08_15.lb007.addedTests);
  return normalized;
}

export function hasExactNestedProjectPowershellWorkspaceWriteAmendment20260815(contractsDoc) {
  if (contractsDoc?.schema_version !== NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.schemaVersion) return false;
  for (const [key, expected] of Object.entries(NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb006 = contractsDoc?.prs?.["LB-006"];
  const lb007 = contractsDoc?.prs?.["LB-007"];
  if (!lb006 || !lb007) return false;
  return containsAll(lb006.required_artifacts, NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.lb006.addedArtifacts)
    && containsAll(lb006.required_tests, NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.lb006.addedTests)
    && containsAll(lb007.required_artifacts, NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.lb007.addedArtifacts)
    && containsAll(lb007.required_tests, NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.lb007.addedTests);
}

export function normalizeNestedProjectPowershellWorkspaceWriteAmendment20260815(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.baselineSchemaVersion;
  for (const key of Object.keys(NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.addedRules)) delete normalized.rules[key];
  const lb006 = normalized.prs?.["LB-006"];
  const lb007 = normalized.prs?.["LB-007"];
  if (lb006) {
    lb006.required_artifacts = removeItems(lb006.required_artifacts, NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.lb006.addedArtifacts);
    lb006.required_tests = removeItems(lb006.required_tests, NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.lb006.addedTests);
  }
  if (lb007) {
    lb007.required_artifacts = removeItems(lb007.required_artifacts, NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.lb007.addedArtifacts);
    lb007.required_tests = removeItems(lb007.required_tests, NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.lb007.addedTests);
  }
  return normalized;
}

export function hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(contractsDoc) {
  if (contractsDoc?.schema_version !== TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.schemaVersion) return false;
  for (const [key, expected] of Object.entries(TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb006 = contractsDoc?.prs?.["LB-006"];
  const lb018 = contractsDoc?.prs?.["LB-018"];
  const lb019 = contractsDoc?.prs?.["LB-019"];
  if (!lb006 || !lb018 || !lb019) return false;
  return containsAll(lb006.required_artifacts, TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.lb006.addedArtifacts)
    && containsAll(lb006.required_tests, TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.lb006.addedTests)
    && containsAll(lb018.required_tests, TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.lb018.addedTests)
    && containsAll(lb019.required_tests, TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.lb019.addedTests);
}

export function normalizeTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.baselineSchemaVersion;
  for (const key of Object.keys(TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.addedRules)) delete normalized.rules[key];
  const lb006 = normalized.prs?.["LB-006"];
  const lb018 = normalized.prs?.["LB-018"];
  const lb019 = normalized.prs?.["LB-019"];
  if (lb006) {
    lb006.required_artifacts = removeItems(lb006.required_artifacts, TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.lb006.addedArtifacts);
    lb006.required_tests = removeItems(lb006.required_tests, TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.lb006.addedTests);
  }
  if (lb018) lb018.required_tests = removeItems(lb018.required_tests, TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.lb018.addedTests);
  if (lb019) lb019.required_tests = removeItems(lb019.required_tests, TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.lb019.addedTests);
  return normalized;
}

export function hasExactPublicFacadeRuntimeSemanticsAmendment20260814(contractsDoc) {
  if (contractsDoc?.schema_version !== PUBLIC_FACADE_RUNTIME_SEMANTICS_AMENDMENT_2026_08_14.schemaVersion) return false;
  for (const [key, expected] of Object.entries(PUBLIC_FACADE_RUNTIME_SEMANTICS_AMENDMENT_2026_08_14.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb006 = contractsDoc?.prs?.["LB-006"];
  if (!lb006) return false;
  return containsAll(lb006.required_artifacts, PUBLIC_FACADE_RUNTIME_SEMANTICS_AMENDMENT_2026_08_14.lb006.addedArtifacts)
    && containsAll(lb006.required_tests, PUBLIC_FACADE_RUNTIME_SEMANTICS_AMENDMENT_2026_08_14.lb006.addedTests);
}

export function normalizePublicFacadeRuntimeSemanticsAmendment20260814(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = PUBLIC_FACADE_RUNTIME_SEMANTICS_AMENDMENT_2026_08_14.baselineSchemaVersion;
  for (const key of Object.keys(PUBLIC_FACADE_RUNTIME_SEMANTICS_AMENDMENT_2026_08_14.addedRules)) delete normalized.rules[key];
  const lb006 = normalized.prs?.["LB-006"];
  if (lb006) {
    lb006.required_artifacts = removeItems(lb006.required_artifacts, PUBLIC_FACADE_RUNTIME_SEMANTICS_AMENDMENT_2026_08_14.lb006.addedArtifacts);
    lb006.required_tests = removeItems(lb006.required_tests, PUBLIC_FACADE_RUNTIME_SEMANTICS_AMENDMENT_2026_08_14.lb006.addedTests);
  }
  return normalized;
}

export function hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(contractsDoc) {
  if (contractsDoc?.schema_version !== LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.schemaVersion) return false;
  for (const [key, expected] of Object.entries(LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.addedRules)) {
    if (canonicalJson(contractsDoc?.rules?.[key]) !== canonicalJson(expected)) return false;
  }
  const lb006 = contractsDoc?.prs?.["LB-006"];
  const lb007 = contractsDoc?.prs?.["LB-007"];
  if (!lb006 || !lb007) return false;
  const delta6 = LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.lb006;
  const delta7 = LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.lb007;
  return containsAll(lb006.writable_paths, delta6.addedWritablePaths)
    && containsAll(lb006.required_artifacts, delta6.addedArtifacts)
    && containsAll(lb006.required_tests, delta6.addedTests)
    && containsAll(lb006.non_goals, delta6.addedNonGoals)
    && containsAll(lb007.required_artifacts, delta7.addedArtifacts)
    && containsAll(lb007.required_tests, delta7.addedTests);
}

export function normalizeLocalBridgeAgentRuntimeFacadeAmendment20260814(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.baselineSchemaVersion;
  for (const key of Object.keys(LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.addedRules)) delete normalized.rules[key];
  const lb006 = normalized.prs?.["LB-006"];
  const lb007 = normalized.prs?.["LB-007"];
  if (lb006) {
    const delta = LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.lb006;
    lb006.writable_paths = removeItems(lb006.writable_paths, delta.addedWritablePaths);
    lb006.required_artifacts = removeItems(lb006.required_artifacts, delta.addedArtifacts);
    lb006.required_tests = removeItems(lb006.required_tests, delta.addedTests);
    lb006.non_goals = removeItems(lb006.non_goals, delta.addedNonGoals);
  }
  if (lb007) {
    const delta = LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.lb007;
    lb007.required_artifacts = removeItems(lb007.required_artifacts, delta.addedArtifacts);
    lb007.required_tests = removeItems(lb007.required_tests, delta.addedTests);
  }
  return normalized;
}

export function hasExactG3UiFirstScrollbarAmendment20260814(contractsDoc) {
  if (contractsDoc?.schema_version !== G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.schemaVersion) return false;
  const rules = contractsDoc?.rules;
  for (const [key, expected] of Object.entries(G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.addedRules)) {
    if (JSON.stringify(rules?.[key]) !== JSON.stringify(expected)) return false;
  }
  const lb014 = contractsDoc?.prs?.["LB-014"];
  const lb015 = contractsDoc?.prs?.["LB-015"];
  if (!lb014 || !lb015) return false;
  for (const [current, old] of G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.lb014.artifactReplacements) {
    if (!lb014.required_artifacts?.includes(current) || lb014.required_artifacts?.includes(old)) return false;
  }
  for (const [current, old] of G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.lb014.testReplacements) {
    if (!lb014.required_tests?.includes(current) || lb014.required_tests?.includes(old)) return false;
  }
  if (!containsAll(lb014.required_tests, G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.lb014.addedTests)) return false;
  if (!containsAll(lb015.required_artifacts, G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.lb015.addedArtifacts)) return false;
  if (!containsAll(lb015.required_tests, G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.lb015.addedTests)) return false;
  return true;
}

export function normalizeG3UiFirstScrollbarAmendment20260814(contractsDoc) {
  const normalized = structuredClone(contractsDoc ?? null);
  if (!normalized) return normalized;
  normalized.schema_version = G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.baselineSchemaVersion;
  for (const key of Object.keys(G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.addedRules)) delete normalized.rules[key];
  const lb014 = normalized.prs?.["LB-014"];
  if (lb014) {
    lb014.required_artifacts = lb014.required_artifacts
      .map((item) => G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.lb014.artifactReplacements.find(([current]) => current === item)?.[1] ?? item);
    lb014.required_tests = removeItems(lb014.required_tests, G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.lb014.addedTests)
      .map((item) => G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.lb014.testReplacements.find(([current]) => current === item)?.[1] ?? item);
  }
  const lb015 = normalized.prs?.["LB-015"];
  if (lb015) {
    lb015.required_artifacts = removeItems(lb015.required_artifacts, G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.lb015.addedArtifacts);
    lb015.required_tests = removeItems(lb015.required_tests, G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.lb015.addedTests);
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
  if ((authorizationContracts?.schema_version ?? 0) >= PUBLIC_MCP_OUTPUT_SCHEMA_AMENDMENT_2026_08_16.schemaVersion) {
    if (!hasExactPublicMcpOutputSchemaAmendment20260816(authorizationContracts)) {
      findings.push(`${expected.id}:public-mcp-output-schema-20260816-contract-amendment-drift`);
    }
    authorizationContracts = normalizePublicMcpOutputSchemaAmendment20260816(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= G3_UI_TRAY_REFINEMENT_AMENDMENT_2026_08_16.schemaVersion) {
    if (!hasExactG3UiTrayRefinementAmendment20260816(authorizationContracts)) {
      findings.push(`${expected.id}:g3-ui-tray-refinement-20260816-contract-amendment-drift`);
    }
    authorizationContracts = normalizeG3UiTrayRefinementAmendment20260816(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= PERMISSION_MODE_EQUAL_THIRDS_GEOMETRY_AMENDMENT_2026_08_16.schemaVersion) {
    if (!hasExactPermissionModeEqualThirdsGeometryAmendment20260816(authorizationContracts)) {
      findings.push(`${expected.id}:permission-mode-equal-thirds-20260816-contract-amendment-drift`);
    }
    authorizationContracts = normalizePermissionModeEqualThirdsGeometryAmendment20260816(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= UI_GREEN_STORAGE_DEFAULT_NON_ADMIN_AMENDMENT_2026_08_16.schemaVersion) {
    if (!hasExactUiGreenStorageDefaultNonAdminAmendment20260816(authorizationContracts)) {
      findings.push(`${expected.id}:ui-green-storage-default-non-admin-20260816-contract-amendment-drift`);
    }
    authorizationContracts = normalizeUiGreenStorageDefaultNonAdminAmendment20260816(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= PERMISSION_EXECUTION_MODEL_AMENDMENT_2026_08_16.schemaVersion) {
    if (!hasExactPermissionExecutionModelAmendment20260816(authorizationContracts)) {
      findings.push(`${expected.id}:permission-execution-model-20260816-contract-amendment-drift`);
    }
    authorizationContracts = normalizePermissionExecutionModelAmendment20260816(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= ELEVATED_SCOPE_AND_UI_GEOMETRY_AMENDMENT_2026_08_15.schemaVersion) {
    if (!hasExactElevatedScopeAndUiGeometryAmendment20260815(authorizationContracts)) {
      findings.push(`${expected.id}:elevated-scope-ui-geometry-20260815-contract-amendment-drift`);
    }
    authorizationContracts = normalizeElevatedScopeAndUiGeometryAmendment20260815(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= WINDOWS_SYSTEM_MANAGEMENT_PRIVILEGE_AMENDMENT_2026_08_15.schemaVersion) {
    if (!hasExactWindowsSystemManagementPrivilegeAmendment20260815(authorizationContracts)) {
      findings.push(`${expected.id}:windows-system-management-privilege-20260815-contract-amendment-drift`);
    }
    authorizationContracts = normalizeWindowsSystemManagementPrivilegeAmendment20260815(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= LB007_POLICY_AND_CMD_CODEPAGE_AMENDMENT_2026_08_15.schemaVersion) {
    if (!hasExactLb007PolicyAndCmdCodepageAmendment20260815(authorizationContracts)) {
      findings.push(`${expected.id}:lb007-policy-cmd-codepage-20260815-contract-amendment-drift`);
    }
    authorizationContracts = normalizeLb007PolicyAndCmdCodepageAmendment20260815(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= NESTED_PROJECT_POWERSHELL_WORKSPACE_WRITE_AMENDMENT_2026_08_15.schemaVersion) {
    if (!hasExactNestedProjectPowershellWorkspaceWriteAmendment20260815(authorizationContracts)) {
      findings.push(`${expected.id}:nested-project-powershell-workspace-write-20260815-contract-amendment-drift`);
    }
    authorizationContracts = normalizeNestedProjectPowershellWorkspaceWriteAmendment20260815(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= COMMAND_TASK_STATE_AND_WINDOW_CENTER_AMENDMENT_2026_08_15.schemaVersion) {
    if (!hasExactCommandTaskStateAndWindowCenterAmendment20260815(authorizationContracts)) {
      findings.push(`${expected.id}:command-task-state-window-center-20260815-contract-amendment-drift`);
    }
    authorizationContracts = normalizeCommandTaskStateAndWindowCenterAmendment20260815(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= TEST_ORCHESTRATION_AND_PUBLIC_RUNTIME_CORRECTIONS_AMENDMENT_2026_08_14.schemaVersion) {
    if (!hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(authorizationContracts)) {
      findings.push(`${expected.id}:test-orchestration-public-runtime-corrections-20260814-contract-amendment-drift`);
    }
    authorizationContracts = normalizeTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= PUBLIC_FACADE_RUNTIME_SEMANTICS_AMENDMENT_2026_08_14.schemaVersion) {
    if (!hasExactPublicFacadeRuntimeSemanticsAmendment20260814(authorizationContracts)) {
      findings.push(`${expected.id}:public-facade-runtime-semantics-20260814-contract-amendment-drift`);
    }
    authorizationContracts = normalizePublicFacadeRuntimeSemanticsAmendment20260814(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= ADMIN_MODE_SAFETY_WARNING_AMENDMENT_2026_08_14.schemaVersion) {
    if (!hasExactAdminModeSafetyWarningAmendment20260814(authorizationContracts)) {
      findings.push(`${expected.id}:admin-mode-safety-warning-20260814-contract-amendment-drift`);
    }
    authorizationContracts = normalizeAdminModeSafetyWarningAmendment20260814(authorizationContracts);
  }
  if ((authorizationContracts?.schema_version ?? 0) >= LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.schemaVersion) {
    if (!hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(authorizationContracts)) {
      findings.push(`${expected.id}:agent-runtime-facade-20260814-contract-amendment-drift`);
    }
    const baselineCommit = LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.baselineCommit;
    const baseline = git.commitExists(baselineCommit) && git.isAncestor(baselineCommit)
      ? git.jsonAt(baselineCommit, "PR_CONTRACTS.json")
      : null;
    const normalized = normalizeLocalBridgeAgentRuntimeFacadeAmendment20260814(authorizationContracts);
    if (baseline?.schema_version !== LOCALBRIDGE_AGENT_RUNTIME_FACADE_AMENDMENT_2026_08_14.baselineSchemaVersion) {
      findings.push(`${expected.id}:agent-runtime-facade-20260814-baseline`);
    } else {
      if (JSON.stringify(normalized?.prs) !== JSON.stringify(baseline.prs)) {
        findings.push(`${expected.id}:agent-runtime-facade-20260814-pr-drift`);
      }
      if (canonicalJson(normalized?.rules) !== canonicalJson(baseline.rules)) {
        findings.push(`${expected.id}:agent-runtime-facade-20260814-rule-drift`);
      }
    }
    authorizationContracts = normalized;
  }
  if ((authorizationContracts?.schema_version ?? 0) >= G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.schemaVersion) {
    if (!hasExactG3UiFirstScrollbarAmendment20260814(authorizationContracts)) {
      findings.push(`${expected.id}:ui-first-scrollbar-20260814-contract-amendment-drift`);
    }
    const schema24BaselineCommit = G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.baselineCommit;
    const schema24Baseline = git.commitExists(schema24BaselineCommit) && git.isAncestor(schema24BaselineCommit)
      ? git.jsonAt(schema24BaselineCommit, "PR_CONTRACTS.json")
      : null;
    const normalizedSchema24 = normalizeG3UiFirstScrollbarAmendment20260814(authorizationContracts);
    if (schema24Baseline?.schema_version !== G3_UI_FIRST_SCROLLBAR_AMENDMENT_2026_08_14.baselineSchemaVersion) {
      findings.push(`${expected.id}:ui-first-scrollbar-20260814-baseline`);
    } else {
      if (JSON.stringify(normalizedSchema24?.prs) !== JSON.stringify(schema24Baseline.prs)) {
        findings.push(`${expected.id}:ui-first-scrollbar-20260814-pr-drift`);
      }
      if (canonicalJson(normalizedSchema24?.rules) !== canonicalJson(schema24Baseline.rules)) {
        findings.push(`${expected.id}:ui-first-scrollbar-20260814-rule-drift`);
      }
    }
    authorizationContracts = normalizedSchema24;
  }
  if ((authorizationContracts?.schema_version ?? 0) >= G3_MANUAL_REVIEW_ROUND2_2026_08_14.schemaVersion) {
    if (!hasExactG3ManualReviewRound2_20260814(authorizationContracts)) {
      findings.push(`${expected.id}:manual-review-round2-20260814-contract-amendment-drift`);
    }
    const round2BaselineCommit = G3_MANUAL_REVIEW_ROUND2_2026_08_14.baselineCommit;
    const round2Baseline = git.commitExists(round2BaselineCommit) && git.isAncestor(round2BaselineCommit)
      ? git.jsonAt(round2BaselineCommit, "PR_CONTRACTS.json")
      : null;
    const normalizedRound2 = normalizeG3ManualReviewRound2_20260814(authorizationContracts);
    if (round2Baseline?.schema_version !== G3_MANUAL_REVIEW_ROUND2_2026_08_14.baselineSchemaVersion) {
      findings.push(`${expected.id}:manual-review-round2-20260814-baseline`);
    } else {
      if (JSON.stringify(normalizedRound2?.prs) !== JSON.stringify(round2Baseline.prs)) {
        findings.push(`${expected.id}:manual-review-round2-20260814-pr-drift`);
      }
      if (canonicalJson(normalizedRound2?.rules) !== canonicalJson(round2Baseline.rules)) {
        findings.push(`${expected.id}:manual-review-round2-20260814-rule-drift`);
      }
    }
    authorizationContracts = normalizedRound2;
  }
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
