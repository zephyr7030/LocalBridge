import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { PRE_G4_GATE_AUTHORIZATION, hasExactAdminModeSafetyWarningAmendment20260814, hasExactCommandTaskStateAndWindowCenterAmendment20260815, hasExactG3HumanReviewAmendment, hasExactG3HumanReviewGeneration2Amendment, hasExactG3ManualPathExecutionCorrection20260814, hasExactG3ManualReviewRound2_20260814, hasExactG3ManualSupplement20260814, hasExactG3UiFirstScrollbarAmendment20260814, hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814, hasExactNestedProjectPowershellWorkspaceWriteAmendment20260815, hasExactPublicFacadeRuntimeSemanticsAmendment20260814, hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814, normalizeAdminModeSafetyWarningAmendment20260814, normalizeCommandTaskStateAndWindowCenterAmendment20260815, normalizeG3HumanReviewAmendment, normalizeG3HumanReviewGeneration2Amendment, normalizeG3ManualPathExecutionCorrection20260814, normalizeG3ManualReviewRound2_20260814, normalizeG3ManualSupplement20260814, normalizeG3UiFirstScrollbarAmendment20260814, normalizeLocalBridgeAgentRuntimeFacadeAmendment20260814, normalizeNestedProjectPowershellWorkspaceWriteAmendment20260815, normalizePublicFacadeRuntimeSemanticsAmendment20260814, normalizeTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814, validateG4HumanGate, validatePreG4GateAuthorization } from "./g4-human-gate.mjs";

const evidenceCommit = "a".repeat(40);
const implementationCommit = "b".repeat(40);
const reviewCommit = "c".repeat(40);
const humanDecisionCommit = "d".repeat(40);
const evidence = [
  "G3 全部 PR 完成并通过独立对抗性智能体审查后，不得直接开启 G4。",
  "必须进入人工实测细审核。",
  "有权质疑、拒绝直接采信、要求复核或独立验证执行智能体。",
  "用户和执行智能体的陈述均属于待验证证据。",
  "authorization_id scope actions evidence_ref recorded_by user_audit_status",
  "不得扩大任何普通 PR writable_paths。",
  "唯一的紧随治理实现提交。",
].join("\n");
const expected = {
  ...PRE_G4_GATE_AUTHORIZATION,
  evidenceCommit,
  evidenceCanonicalSha256: createHash("sha256").update(evidence, "utf8").digest("hex"),
  authorizedPaths: ["PR_CONTRACTS.json", "PR_INDEX.json", "scripts/verify-architecture/g4-human-gate.mjs"],
};
const beforePrContracts = { prs: {
  "LB-016": { required_artifacts: ["6-screen wizard", "five-screen onboarding flow", "runtime-check gated confirm button"] },
  "LB-018": { writable_paths: ["src-tauri/**"] },
} };
const contracts = {
  prs: {
    "LB-016": { required_artifacts: ["five-screen onboarding flow", "runtime-check gated confirm button"] },
    "LB-018": { writable_paths: ["src-tauri/**"] },
  },
  rules: { governance_authorizations: [{
    id: expected.id,
    scheme: expected.scheme,
    evidence_path: expected.evidencePath,
    evidence_commit: expected.evidenceCommit,
    evidence_canonical_sha256: expected.evidenceCanonicalSha256,
    scope: expected.scope,
    authorized_paths: expected.authorizedPaths,
    consumed: true,
    does_not_expand_pr_writable_paths: true,
  }] },
};
const authGit = {
  commitExists: (commit) => [evidenceCommit, implementationCommit].includes(commit),
  isAncestor: (commit) => [evidenceCommit, implementationCommit].includes(commit),
  commitPaths: (commit) => commit === evidenceCommit ? [expected.evidencePath] : commit === implementationCommit ? expected.authorizedPaths : [],
  textAt: (commit, path) => commit === evidenceCommit && path === expected.evidencePath ? evidence : null,
  workingText: (path) => path === expected.evidencePath ? evidence : null,
  firstParentChild: (commit) => commit === evidenceCommit ? implementationCommit : null,
  firstParentParent: (commit) => commit === implementationCommit ? evidenceCommit : null,
  jsonAt: (revision, path) => revision === evidenceCommit && path === "PR_CONTRACTS.json" ? beforePrContracts : null,
};
assert.deepEqual(validatePreG4GateAuthorization(contracts, authGit, expected, null), []);
const widenedContracts = structuredClone(contracts);
widenedContracts.prs["LB-018"].writable_paths.push("src/**");
assert.match(validatePreG4GateAuthorization(widenedContracts, authGit, expected, null).join("|"), /ordinary-pr-contract-drift/);
const widenedLb016 = structuredClone(contracts);
widenedLb016.prs["LB-016"].required_artifacts.push("unexpected sixth-screen replacement");
assert.match(validatePreG4GateAuthorization(widenedLb016, authGit, expected, null).join("|"), /ordinary-pr-contract-drift/);
const badAuthGit = { ...authGit, commitPaths: (commit) => commit === implementationCommit ? [...expected.authorizedPaths, "src-tauri/src/lib.rs"] : authGit.commitPaths(commit) };
assert.match(validatePreG4GateAuthorization(contracts, badAuthGit, expected, null).join("|"), /implementation-child-scope/);

const ratificationCommit = "e".repeat(40);
const ratification = {
  commit: ratificationCommit,
  paths: ["AGENTS.md", "PR_CONTRACTS.json", "PR_INDEX.json"],
};
const ratifiedContracts = structuredClone(contracts);
ratifiedContracts.schema_version = 18;
ratifiedContracts.rules.onboarding_screen_count = 6;
ratifiedContracts.rules.onboarding_screen_7_forbidden = true;
ratifiedContracts.rules.ui_button_visible_affordance_required = true;
ratifiedContracts.rules.ui_white_on_white_ambiguous_button_forbidden = true;
ratifiedContracts.rules.ui_minimum_prompt_required = true;
ratifiedContracts.prs["LB-015"] = {
  required_artifacts: ["coherent visible button token system shared across product UI"],
};
ratifiedContracts.prs["LB-016"] = {
  required_artifacts: ["six-screen onboarding flow"],
  required_tests: [
    "onboarding has exactly six screens",
    "screen 4 custom connector setup button",
    "system browser allowlist is the exact fixed ChatGPT custom connector URL",
    "screen 4 opens only https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins in the system default browser",
    "screen 4 custom connector guidance is concise and foolproof",
    "screen 6 success message is 设置完成，尝试在 ChatGPT 中选择刚刚添加的连接器吧！",
  ],
};
const ratifiedGit = {
  ...authGit,
  commitExists: (commit) => commit === ratificationCommit || authGit.commitExists(commit),
  isAncestor: (commit) => commit === ratificationCommit || authGit.isAncestor(commit),
  commitPaths: (commit) => commit === ratificationCommit ? ratification.paths : authGit.commitPaths(commit),
  jsonAt: (revision, path) => revision === ratificationCommit && path === "PR_CONTRACTS.json" ? ratifiedContracts : authGit.jsonAt(revision, path),
};
assert.deepEqual(validatePreG4GateAuthorization(ratifiedContracts, ratifiedGit, expected, ratification), []);
const authorizedLb016Rework = structuredClone(ratifiedContracts);
authorizedLb016Rework.prs["LB-016"].required_artifacts.push(
  "fixed 900x620 non-resizable non-maximizable main window",
  "single edge-to-edge custom window chrome with native decorations disabled",
  "full-page onboarding layout using the fixed custom-chrome content area",
);
authorizedLb016Rework.prs["LB-016"].required_tests = authorizedLb016Rework.prs["LB-016"].required_tests
  .map((item) => item === "screen 6 success message is 设置完成，尝试在 ChatGPT 中选择刚刚添加的连接器吧！"
    ? "screen 6 success message is 配置完成，在插件中选择刚刚添加的Local Bridge试试吧"
    : item);
authorizedLb016Rework.prs["LB-016"].required_tests.push(
  "screens 4 and 5 use Local Bridge as the user-facing connector term",
  "main window is fixed to 900x620 with minimum and maximum 900x620 resizable false and maximizable false",
  "native window decorations are disabled and exactly one edge-to-edge custom chrome provides drag minimize and close without maximize or double frame",
  "onboarding uses the full fixed client content area without a centered floating card modal shell or large empty surrounding canvas",
);
assert.deepEqual(validatePreG4GateAuthorization(authorizedLb016Rework, ratifiedGit, expected, ratification), []);
const finalLb016Rework = structuredClone(authorizedLb016Rework);
finalLb016Rework.prs["LB-016"].required_artifacts.push(
  "screen 4 create-custom-plugin persisted-information panel",
);
finalLb016Rework.prs["LB-016"].required_tests = finalLb016Rework.prs["LB-016"].required_tests.map((item) => {
  if (item === "screen 4 custom connector setup button") return "screen 4 lower plugin action label is 打开插件管理页";
  if (item === "screen 4 opens only https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins in the system default browser") return "screen 4 plugin management action opens only https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins in the system default browser through a fixed Rust allowlist";
  if (item === "screen 4 custom connector guidance is concise and foolproof") return "screen 4 developer-mode guidance is 在插件设置页面最底端，打开“开发者模式”";
  return item;
});
finalLb016Rework.prs["LB-016"].required_tests.push(
  "screen 4 title is 创建自定义插件",
  "screen 4 ChatGPT plugin-settings action opens only https://chatgpt.com/plugins#settings/Plugins in the system default browser through a fixed Rust allowlist",
  "screen 4 shows exactly two concise information rows 名称 Tunnel ID and does not show 本地服务",
  "screen 4 名称 value is Local Bridge",
  "screen 4 Tunnel ID value reflects the current persisted saved value rather than an unsaved frontend-only value",
  "each of the two screen 4 information rows has its own copy action and successful copy shows green 已复制 for exactly 3 seconds before restoring without layout shift",
  "screen 3 permission mode buttons preserve clearly visible balanced content-to-border spacing in the real 900x620 render and grow safely for wrapped descriptive text; final visual PASS requires human inspection and cannot be inferred from CSS padding markers alone",
  "the selected project runtime MCP and OpenAI Tunnel are all ready before screen 4 becomes reachable so plugin creation is executable rather than premature guidance",
  "onboarding never defers its only runtime startup edge until screen 5 or screen 6",
);
assert.deepEqual(validatePreG4GateAuthorization(finalLb016Rework, ratifiedGit, expected, ratification), []);
const alteredPluginSettingsUrl = structuredClone(finalLb016Rework);
alteredPluginSettingsUrl.prs["LB-016"].required_tests = alteredPluginSettingsUrl.prs["LB-016"].required_tests
  .map((item) => item.startsWith("screen 4 ChatGPT plugin-settings action opens only ")
    ? "screen 4 ChatGPT plugin-settings action opens an unauthorized URL"
    : item);
assert.match(validatePreG4GateAuthorization(alteredPluginSettingsUrl, ratifiedGit, expected, ratification).join("|"), /ordinary-pr-contract-drift/);
const alteredPluginManagementUrl = structuredClone(finalLb016Rework);
alteredPluginManagementUrl.prs["LB-016"].required_tests = alteredPluginManagementUrl.prs["LB-016"].required_tests
  .map((item) => item.startsWith("screen 4 plugin management action opens only ")
    ? "screen 4 plugin management action opens only https://example.invalid/manage in the system default browser through a fixed Rust allowlist"
    : item);
assert.match(validatePreG4GateAuthorization(alteredPluginManagementUrl, ratifiedGit, expected, ratification).join("|"), /ordinary-pr-contract-drift/);
const alteredInformationRows = structuredClone(finalLb016Rework);
alteredInformationRows.prs["LB-016"].required_tests = alteredInformationRows.prs["LB-016"].required_tests
  .map((item) => item.startsWith("screen 4 shows exactly two concise information rows")
    ? "screen 4 shows three information rows including local service"
    : item);
assert.match(validatePreG4GateAuthorization(alteredInformationRows, ratifiedGit, expected, ratification).join("|"), /ordinary-pr-contract-drift/);
const alteredScreen4Title = structuredClone(finalLb016Rework);
alteredScreen4Title.prs["LB-016"].required_tests = alteredScreen4Title.prs["LB-016"].required_tests
  .map((item) => item === "screen 4 title is 创建自定义插件" ? "screen 4 title is Local Bridge 设置" : item);
assert.match(validatePreG4GateAuthorization(alteredScreen4Title, ratifiedGit, expected, ratification).join("|"), /ordinary-pr-contract-drift/);
const alteredReadinessEdge = structuredClone(finalLb016Rework);
alteredReadinessEdge.prs["LB-016"].required_tests = alteredReadinessEdge.prs["LB-016"].required_tests
  .map((item) => item.startsWith("the selected project runtime MCP and OpenAI Tunnel")
    ? "runtime may start after Screen 4"
    : item);
assert.match(validatePreG4GateAuthorization(alteredReadinessEdge, ratifiedGit, expected, ratification).join("|"), /ordinary-pr-contract-drift/);
const alteredFixedWindowRequirement = structuredClone(authorizedLb016Rework);
alteredFixedWindowRequirement.prs["LB-016"].required_tests = alteredFixedWindowRequirement.prs["LB-016"].required_tests
  .map((item) => item.startsWith("main window is fixed to 900x620")
    ? "main window has an unauthorized variable-size contract"
    : item);
assert.match(validatePreG4GateAuthorization(alteredFixedWindowRequirement, ratifiedGit, expected, ratification).join("|"), /ordinary-pr-contract-drift/);
const alteredAuthorizedCopy = structuredClone(authorizedLb016Rework);
alteredAuthorizedCopy.prs["LB-016"].required_tests = alteredAuthorizedCopy.prs["LB-016"].required_tests
  .map((item) => item.startsWith("screen 6 success message is ")
    ? "screen 6 success message is unauthorized wording"
    : item);
assert.match(validatePreG4GateAuthorization(alteredAuthorizedCopy, ratifiedGit, expected, ratification).join("|"), /ordinary-pr-contract-drift/);
const widenedAuthorizedLb016 = structuredClone(authorizedLb016Rework);
widenedAuthorizedLb016.prs["LB-016"].required_tests.push("unrelated authorized-looking requirement");
assert.match(validatePreG4GateAuthorization(widenedAuthorizedLb016, ratifiedGit, expected, ratification).join("|"), /ordinary-pr-contract-drift/);
const postRatificationDrift = structuredClone(ratifiedContracts);
postRatificationDrift.prs["LB-018"].writable_paths.push("src/**");
assert.match(validatePreG4GateAuthorization(postRatificationDrift, ratifiedGit, expected, ratification).join("|"), /ordinary-pr-contract-drift/);
const badRatificationGit = { ...ratifiedGit, commitPaths: (commit) => commit === ratificationCommit ? [...ratification.paths, "src-tauri/src/lib.rs"] : ratifiedGit.commitPaths(commit) };
assert.match(validatePreG4GateAuthorization(ratifiedContracts, badRatificationGit, expected, ratification).join("|"), /later-contract-ratification-commit-scope/);

const humanReviewAmendmentFixture = {
  "LB-015": {
    required_artifacts: [
      "blue #0071e3 standard product accent with amber administrator-mode exception",
      "shared typed service-status dot presentation for Dashboard and onboarding",
    ],
    required_tests: [
      "primary and ordinary selected controls use the blue #0071e3 accent rather than black",
      "administrator mode uses amber logical selection styling and is not overridden by ordinary blue selected styling",
      "Dashboard tunnel and coding service states render status dots using Ready green Starting amber Fault red Unknown gray semantics from the same typed status source used by onboarding",
      "Dashboard does not maintain an independent conflicting service-status color state",
    ],
  },
  "LB-016": {
    required_artifacts: ["five-screen onboarding flow"],
    required_tests: [
      "screen 4 ChatGPT plugin-settings action is placed in the left-side action flow",
      "screen 4 shows 打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件 beneath the plugin-settings action",
      "screen 4 lower plugin-management action is placed in the left-side action flow",
      "onboarding has exactly five screens",
      "ordinary selected permission modes use the standard blue accent while administrator mode uses amber logical styling in onboarding and Dashboard",
      "onboarding never defers its only runtime startup edge until screen 5",
      "screen 4 provides an explicit back action to screen 3 and a continue action to screen 5",
      "screens 2 3 4 and 5 each provide an explicit back path and save start or configuration failure never traps the user",
      "copy-success feedback reserves layout space and causes no layout shift",
      "screen 5 contains only local runtime environment coding service and OpenAI Tunnel checks",
      "screen 5 status dots map Ready to green Starting to amber Fault to red and Unknown to gray using the shared typed service-status source",
      "screen 5 confirm is disabled until all three checks are green",
      "screen 5 success message is hidden until all three checks are green",
      "screen 5 success message is 配置完成，在插件中选择刚刚添加的Local Bridge试试吧",
      "screen 5 does not auto-advance",
      "screen 5 provides an explicit back action to screen 4",
      "screen 5 confirm enters main UI after readiness",
      "onboarding has no sixth screen",
      "screen 4 uses Local Bridge as the user-facing connector term",
      "primary and ordinary selected wizard controls use the blue #0071e3 accent rather than black",
    ],
  },
};
assert.equal(hasExactG3HumanReviewAmendment(humanReviewAmendmentFixture), true);
const normalizedHumanReview = normalizeG3HumanReviewAmendment(humanReviewAmendmentFixture);
assert.deepEqual(normalizedHumanReview["LB-015"].required_artifacts, []);
assert.deepEqual(normalizedHumanReview["LB-015"].required_tests, []);
assert.deepEqual(normalizedHumanReview["LB-016"].required_artifacts, ["six-screen onboarding flow"]);
for (const restored of [
  "onboarding has exactly six screens",
  "onboarding never defers its only runtime startup edge until screen 5 or screen 6",
  "screen 5 provides only the minimum connector confirmation/use guidance and does not pretend to detect ChatGPT state",
  "if a connector endpoint is displayed or copied it comes from a typed Rust projection backed by verified tunnel or control-plane metadata",
  "frontend never derives a connector endpoint from Tunnel ID or fabricates one",
  "screen 6 contains only local runtime environment coding service and OpenAI Tunnel checks",
  "onboarding has no seventh screen",
]) assert.equal(normalizedHumanReview["LB-016"].required_tests.includes(restored), true);

const changedTunelHint = structuredClone(humanReviewAmendmentFixture);
changedTunelHint["LB-016"].required_tests = changedTunelHint["LB-016"].required_tests.map((item) => item.includes("刚刚添加的Tunel") ? item.replace("刚刚添加的Tunel", "刚刚添加的Tunnel") : item);
assert.equal(hasExactG3HumanReviewAmendment(changedTunelHint), false);
const rolledBackToSix = structuredClone(humanReviewAmendmentFixture);
rolledBackToSix["LB-016"].required_artifacts = ["six-screen onboarding flow"];
assert.equal(hasExactG3HumanReviewAmendment(rolledBackToSix), false);
const changedBackPath = structuredClone(humanReviewAmendmentFixture);
changedBackPath["LB-016"].required_tests = changedBackPath["LB-016"].required_tests.map((item) => item.startsWith("screens 2 3 4 and 5 each provide") ? "screens 2 3 and 4 provide a back path" : item);
assert.equal(hasExactG3HumanReviewAmendment(changedBackPath), false);
const changedTypedStatus = structuredClone(humanReviewAmendmentFixture);
changedTypedStatus["LB-016"].required_tests = changedTypedStatus["LB-016"].required_tests.map((item) => item.startsWith("screen 5 status dots map") ? "screen 5 status dots may use independent boolean state" : item);
assert.equal(hasExactG3HumanReviewAmendment(changedTypedStatus), false);
const changedRuntimeEdge = structuredClone(humanReviewAmendmentFixture);
changedRuntimeEdge["LB-016"].required_tests = changedRuntimeEdge["LB-016"].required_tests.map((item) => item.startsWith("onboarding never defers its only runtime startup edge") ? "onboarding may defer runtime startup until screen 5" : item);
assert.equal(hasExactG3HumanReviewAmendment(changedRuntimeEdge), false);
const schema19FullRollback = structuredClone(ratifiedContracts);
schema19FullRollback.schema_version = 19;
assert.match(validatePreG4GateAuthorization(schema19FullRollback, ratifiedGit, expected, ratification).join("|"), /human-review-contract-amendment-drift/);

const schema30Contracts = JSON.parse(readFileSync(new URL("../../PR_CONTRACTS.json", import.meta.url), "utf8"));
assert.equal(hasExactNestedProjectPowershellWorkspaceWriteAmendment20260815(schema30Contracts), true);
const schema29Contracts = normalizeNestedProjectPowershellWorkspaceWriteAmendment20260815(schema30Contracts);
assert.equal(schema29Contracts.schema_version, 29);
assert.equal(hasExactCommandTaskStateAndWindowCenterAmendment20260815(schema29Contracts), true);
const schema28Contracts = normalizeCommandTaskStateAndWindowCenterAmendment20260815(schema29Contracts);
assert.equal(schema28Contracts.schema_version, 28);
assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28Contracts), true);
const schema27Contracts = normalizeTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28Contracts);
assert.equal(schema27Contracts.schema_version, 27);
assert.equal(hasExactPublicFacadeRuntimeSemanticsAmendment20260814(schema27Contracts), true);
const schema26Contracts = normalizePublicFacadeRuntimeSemanticsAmendment20260814(schema27Contracts);
assert.equal(schema26Contracts.schema_version, 26);
assert.equal(hasExactAdminModeSafetyWarningAmendment20260814(schema26Contracts), true);
const schema26ProjectState = JSON.parse(readFileSync(new URL("../../PROJECT_STATE.json", import.meta.url), "utf8"));
const schema26Ratification = schema26ProjectState.schema26_admin_mode_safety_warning_2026_08_14;
assert.equal(schema26Ratification.contract_review_status, "PASS");
assert.deepEqual(schema26Ratification.owner_prs, ["LB-015", "LB-016"]);
assert.deepEqual(schema26Ratification.current_execution_pointer_unchanged, { current_group: "G2", current_pr: "LB-006" });
assert.equal(schema26ProjectState.permission_architecture.user_controls_enable_disable, false);
assert.equal(schema26ProjectState.permission_architecture.user_controls_permission_mode_selection, true);
assert.equal(schema26ProjectState.permission_architecture.user_controls_broker_directly, false);
const schema28Ratification = schema26ProjectState.schema28_test_orchestration_and_public_runtime_corrections_2026_08_14;
assert.equal(schema28Ratification.ratified, true);
assert.equal(schema28Ratification.owner_pr, "LB-006");
assert.equal(schema28Ratification.reopen_from_pr, "LB-006");
assert.equal(schema28Ratification.next_required_g2_adversarial_generation, 10);
assert.equal(schema28Ratification.g2_generation_10_consumed, false);
assert.equal(schema28Ratification.g3_unlocked, false);
assert.equal(schema28Ratification.g4_unlocked, false);
const schema25Contracts = normalizeAdminModeSafetyWarningAmendment20260814(schema26Contracts);
assert.equal(schema25Contracts.schema_version, 25);
assert.equal(schema25Contracts.rules.ui_admin_mode_logic_accent, "amber");
assert.equal(schema25Contracts.rules.visible_admin_mode_selection_requests_uac, true);
assert.equal(Object.hasOwn(schema25Contracts.rules, "admin_mode_warning_countdown_ms"), false);

const schema26WrongOrange = structuredClone(schema26Contracts);
schema26WrongOrange.rules.ui_admin_mode_logic_accent = "amber";
assert.equal(hasExactAdminModeSafetyWarningAmendment20260814(schema26WrongOrange), false);
const schema26OnlyNumberRed = structuredClone(schema26Contracts);
schema26OnlyNumberRed.rules.admin_mode_warning_confirm_entire_button_red = false;
assert.equal(hasExactAdminModeSafetyWarningAmendment20260814(schema26OnlyNumberRed), false);
const schema26ShortCountdown = structuredClone(schema26Contracts);
schema26ShortCountdown.rules.admin_mode_warning_countdown_ms = 8000;
assert.equal(hasExactAdminModeSafetyWarningAmendment20260814(schema26ShortCountdown), false);
const schema26CopyDrift = structuredClone(schema26Contracts);
schema26CopyDrift.rules.admin_mode_warning_consequences[7] = "系统可能需要修复";
assert.equal(hasExactAdminModeSafetyWarningAmendment20260814(schema26CopyDrift), false);
const schema26CountdownEnabledEarly = structuredClone(schema26Contracts);
schema26CountdownEnabledEarly.rules.admin_mode_warning_confirm_disabled_during_countdown = false;
assert.equal(hasExactAdminModeSafetyWarningAmendment20260814(schema26CountdownEnabledEarly), false);
const schema26CancelSideEffect = structuredClone(schema26Contracts);
schema26CancelSideEffect.rules.admin_mode_warning_cancel_escape_dismiss_no_side_effect = false;
assert.equal(hasExactAdminModeSafetyWarningAmendment20260814(schema26CancelSideEffect), false);
const schema26DirectUac = structuredClone(schema26Contracts);
schema26DirectUac.rules.admin_mode_uac_before_enabled_confirm_forbidden = false;
assert.equal(hasExactAdminModeSafetyWarningAmendment20260814(schema26DirectUac), false);
const schema26BackgroundWarning = structuredClone(schema26Contracts);
schema26BackgroundWarning.rules.admin_mode_background_restore_warning_forbidden = false;
assert.equal(hasExactAdminModeSafetyWarningAmendment20260814(schema26BackgroundWarning), false);
const schema26DuplicateUac = structuredClone(schema26Contracts);
schema26DuplicateUac.rules.admin_mode_active_broker_reselection_duplicate_uac_forbidden = false;
assert.equal(hasExactAdminModeSafetyWarningAmendment20260814(schema26DuplicateUac), false);

for (const rule of [
  "command_terminal_unconditional_finalizer_required",
  "command_terminal_finalizer_must_atomically_append_finished_and_clear_current",
  "command_terminal_finished_event_exactly_once_required",
  "task_state_terminal_snapshot_persistence_required",
  "task_state_terminal_truth_independent_of_private_session_retention_required",
  "task_state_command_owner_compare_and_swap_required",
  "task_state_non_owner_overwrite_or_clear_forbidden",
  "task_state_duplicate_terminal_finalization_idempotent",
  "main_window_default_centered_required",
  "existing_window_reopen_forced_recenter_forbidden",
]) {
  const weakened = structuredClone(schema29Contracts);
  weakened.rules[rule] = !schema29Contracts.rules[rule];
  assert.equal(hasExactCommandTaskStateAndWindowCenterAmendment20260815(weakened), false, rule);
}
const schema29OwnerIdentityDrift = structuredClone(schema29Contracts);
schema29OwnerIdentityDrift.rules.task_state_command_owner_identity = ["session_id"];
assert.equal(hasExactCommandTaskStateAndWindowCenterAmendment20260815(schema29OwnerIdentityDrift), false);
const schema29MissingFinallyProof = structuredClone(schema29Contracts);
schema29MissingFinallyProof.prs["LB-006"].required_tests = schema29MissingFinallyProof.prs["LB-006"].required_tests.filter((item) => !item.startsWith("every command terminal path"));
assert.equal(hasExactCommandTaskStateAndWindowCenterAmendment20260815(schema29MissingFinallyProof), false);
const schema29MissingRetentionProof = structuredClone(schema29Contracts);
schema29MissingRetentionProof.prs["LB-006"].required_tests = schema29MissingRetentionProof.prs["LB-006"].required_tests.filter((item) => !item.startsWith("task-state persists a bounded redacted terminal command snapshot"));
assert.equal(hasExactCommandTaskStateAndWindowCenterAmendment20260815(schema29MissingRetentionProof), false);
const schema29MissingOwnerCasProof = structuredClone(schema29Contracts);
schema29MissingOwnerCasProof.prs["LB-006"].required_tests = schema29MissingOwnerCasProof.prs["LB-006"].required_tests.filter((item) => !item.startsWith("task-state command start replace finish and clear operations"));
assert.equal(hasExactCommandTaskStateAndWindowCenterAmendment20260815(schema29MissingOwnerCasProof), false);
const schema29MissingCenterProof = structuredClone(schema29Contracts);
schema29MissingCenterProof.prs["LB-015"].required_tests = schema29MissingCenterProof.prs["LB-015"].required_tests.filter((item) => !item.startsWith("on normal foreground first visible creation"));
assert.equal(hasExactCommandTaskStateAndWindowCenterAmendment20260815(schema29MissingCenterProof), false);

for (const rule of [
  "agent_workflow_workspace_relative_project_path_selector_required",
  "agent_workflow_nested_project_discovery_consistent_with_git_workflow_required",
  "agent_workflow_project_selector_must_not_change_active_workspace_authority",
  "trusted_powershell_standard_cmdlet_surface_required",
  "trusted_powershell_arbitrary_module_autoload_forbidden",
  "trusted_powershell_standard_module_preload_identity_validation_required",
  "workspace_structured_directory_write_required",
  "workspace_structured_directory_write_active_root_only",
  "workspace_structured_directory_write_must_not_mutate_workspace_control_plane",
  "workspace_structured_directory_reparse_escape_forbidden",
  "powershell_provider_mutation_review_requirement_preserved",
  "agent_workflow_structured_directory_changes_field_required",
]) {
  const weakened = structuredClone(schema30Contracts);
  weakened.rules[rule] = !schema30Contracts.rules[rule];
  assert.equal(hasExactNestedProjectPowershellWorkspaceWriteAmendment20260815(weakened), false, rule);
}
const schema30MissingNestedProof = structuredClone(schema30Contracts);
schema30MissingNestedProof.prs["LB-006"].required_tests = schema30MissingNestedProof.prs["LB-006"].required_tests.filter((item) => !item.startsWith("with active workspace D:\\project, agent_workflow path=LocalBridge"));
assert.equal(hasExactNestedProjectPowershellWorkspaceWriteAmendment20260815(schema30MissingNestedProof), false);
const schema30MissingPowershellProof = structuredClone(schema30Contracts);
schema30MissingPowershellProof.prs["LB-006"].required_tests = schema30MissingPowershellProof.prs["LB-006"].required_tests.filter((item) => !item.startsWith("windows_powershell and auto resolving to PowerShell"));
assert.equal(hasExactNestedProjectPowershellWorkspaceWriteAmendment20260815(schema30MissingPowershellProof), false);
const schema30MissingWorkspaceWriteProof = structuredClone(schema30Contracts);
schema30MissingWorkspaceWriteProof.prs["LB-006"].required_tests = schema30MissingWorkspaceWriteProof.prs["LB-006"].required_tests.filter((item) => !item.startsWith("agent_workflow exposes optional directory_changes"));
assert.equal(hasExactNestedProjectPowershellWorkspaceWriteAmendment20260815(schema30MissingWorkspaceWriteProof), false);
const schema30DirectoryActionsDrift = structuredClone(schema30Contracts);
schema30DirectoryActionsDrift.rules.agent_workflow_directory_change_actions = ["create_directory", "remove_empty_directory", "remove_tree"];
assert.equal(hasExactNestedProjectPowershellWorkspaceWriteAmendment20260815(schema30DirectoryActionsDrift), false);

const schema28NoFixtureCompression = structuredClone(schema28Contracts);
schema28NoFixtureCompression.rules.test_heavy_shared_fixture_lifecycle_compression_required = false;
assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28NoFixtureCompression), false);
const schema28ForcesUnitMerge = structuredClone(schema28Contracts);
schema28ForcesUnitMerge.rules.test_cheap_unit_tests_forced_merge_forbidden = false;
assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28ForcesUnitMerge), false);
const schema28StaticCanSubstitute = structuredClone(schema28Contracts);
schema28StaticCanSubstitute.rules.test_static_contract_behavioral_substitution_forbidden = false;
assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28StaticCanSubstitute), false);
const schema28UnboundedLostSession = structuredClone(schema28Contracts);
schema28UnboundedLostSession.rules.test_lost_session_must_terminal_fail = false;
assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28UnboundedLostSession), false);
const schema28DevMustBeConsoleFree = structuredClone(schema28Contracts);
schema28DevMustBeConsoleFree.rules.development_console_window_free_required = true;
assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28DevMustBeConsoleFree), false);
const schema28PackagedConsoleAllowed = structuredClone(schema28Contracts);
schema28PackagedConsoleAllowed.rules.packaged_gui_managed_child_visible_console_forbidden = false;
assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28PackagedConsoleAllowed), false);
for (const rule of [
  "shell_command_exact_single_parse_semantics_required",
  "command_poll_incremental_output_no_loss_no_replay_required",
  "command_write_running_session_required",
  "command_kill_cancel_terminal_convergence_required",
  "view_image_real_auto_resize_required",
  "public_command_output_utf8_required",
  "git_blame_line_range_one_based_inclusive",
  "document_line_range_one_based_inclusive",
  "runtime_result_semantic_probe_all_adapter_consumed_unmodeled_fields_required",
]) {
  const weakened = structuredClone(schema28Contracts);
  weakened.rules[rule] = false;
  assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(weakened), false, rule);
}
const schema28MissingLifecycleE2e = structuredClone(schema28Contracts);
schema28MissingLifecycleE2e.prs["LB-006"].required_tests = schema28MissingLifecycleE2e.prs["LB-006"].required_tests.filter((item) => !item.startsWith("one real public command session lifecycle E2E"));
assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28MissingLifecycleE2e), false);
const schema28MissingKillProof = structuredClone(schema28Contracts);
schema28MissingKillProof.prs["LB-006"].required_tests = schema28MissingKillProof.prs["LB-006"].required_tests.filter((item) => !item.startsWith("command_control.kill on a valid running public session"));
assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28MissingKillProof), false);
const schema28MissingPackagedProof = structuredClone(schema28Contracts);
schema28MissingPackagedProof.prs["LB-018"].required_tests = schema28MissingPackagedProof.prs["LB-018"].required_tests.filter((item) => !item.startsWith("release-style packaged GUI launch"));
assert.equal(hasExactTestOrchestrationAndPublicRuntimeCorrectionsAmendment20260814(schema28MissingPackagedProof), false);

const schema27LeakedPrivateSession = structuredClone(schema27Contracts);
schema27LeakedPrivateSession.rules.upstream_private_session_handles_public_forbidden = false;
assert.equal(hasExactPublicFacadeRuntimeSemanticsAmendment20260814(schema27LeakedPrivateSession), false);
const schema27PollMiswired = structuredClone(schema27Contracts);
schema27PollMiswired.rules.command_control_poll_retained_output_mapping_forbidden = false;
assert.equal(hasExactPublicFacadeRuntimeSemanticsAmendment20260814(schema27PollMiswired), false);
const schema27MissingWorkspaceProbe = structuredClone(schema27Contracts);
schema27MissingWorkspaceProbe.prs["LB-006"].required_tests = schema27MissingWorkspaceProbe.prs["LB-006"].required_tests.filter((item) => !item.startsWith("if private get_default_cwd result semantics"));
assert.equal(hasExactPublicFacadeRuntimeSemanticsAmendment20260814(schema27MissingWorkspaceProbe), false);
const schema27MissingNestedGit = structuredClone(schema27Contracts);
schema27MissingNestedGit.prs["LB-006"].required_artifacts = schema27MissingNestedGit.prs["LB-006"].required_artifacts.filter((item) => !item.startsWith("shared workspace-bounded nested Git repository resolver"));
assert.equal(hasExactPublicFacadeRuntimeSemanticsAmendment20260814(schema27MissingNestedGit), false);

for (const staleDocument of [
  "../../docs/01_PRODUCT_SCOPE.md",
  "../../docs/02_ARCHITECTURE.md",
  "../../docs/03_SECURITY_MODEL.md",
  "../../docs/04_UX_SPEC.md",
  "../../docs/05_RUNTIME_CONTRACT.md",
  "../../docs/06_PR_PLAN.md",
  "../../docs/07_ACCEPTANCE_MATRIX.md",
  "../../docs/08_UPSTREAM_POLICY.md",
  "../../docs/LOCALBRIDGE_FINAL_AGENT_RUNTIME_DESIGN.md",
]) {
  assert.equal(existsSync(new URL(staleDocument, import.meta.url)), false, `superseded document must stay removed: ${staleDocument}`);
}

assert.equal(hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25Contracts), true);
const schema24Contracts = normalizeLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25Contracts);
assert.equal(schema24Contracts.schema_version, 24);
assert.equal(Object.hasOwn(schema24Contracts.rules, "localbridge_agent_api_owned"), false);
assert.equal(schema24Contracts.prs["LB-006"].writable_paths.includes("tests/integration/command/**"), false);
assert.equal(schema24Contracts.prs["LB-007"].required_artifacts.includes("LocalBridge stable public capability/action classifier independent of upstream tool names"), false);

const schema25UpstreamPassthrough = structuredClone(schema25Contracts);
schema25UpstreamPassthrough.rules.upstream_tools_list_public_passthrough_forbidden = false;
assert.equal(hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25UpstreamPassthrough), false);
const schema25RawCoreTool = structuredClone(schema25Contracts);
schema25RawCoreTool.rules.localbridge_agent_api_v1_core_tools.push("read_file");
assert.equal(hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25RawCoreTool), false);
const schema25UnfilteredToolsList = structuredClone(schema25Contracts);
schema25UnfilteredToolsList.rules.public_tools_list_policy_filtered_subset_of_registry_required = false;
assert.equal(hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25UnfilteredToolsList), false);
const schema25NoCapabilityNegotiation = structuredClone(schema25Contracts);
schema25NoCapabilityNegotiation.rules.runtime_capability_negotiation_required = false;
assert.equal(hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25NoCapabilityNegotiation), false);
const schema25RawNamePolicy = structuredClone(schema25Contracts);
schema25RawNamePolicy.rules.policy_classifies_localbridge_capabilities_not_upstream_names = false;
assert.equal(hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25RawNamePolicy), false);
const schema25PathAsTrust = structuredClone(schema25Contracts);
schema25PathAsTrust.rules.shell_path_discovery_is_not_trust_authority = false;
assert.equal(hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25PathAsTrust), false);
const schema25ArbitraryShell = structuredClone(schema25Contracts);
schema25ArbitraryShell.rules.shell_arbitrary_executable_from_mcp_forbidden = false;
assert.equal(hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25ArbitraryShell), false);
const schema25MergedExecutors = structuredClone(schema25Contracts);
schema25MergedExecutors.rules.direct_process_and_shell_execution_separated = false;
assert.equal(hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25MergedExecutors), false);
const schema25MissingPathHijackTest = structuredClone(schema25Contracts);
schema25MissingPathHijackTest.prs["LB-006"].required_tests = schema25MissingPathHijackTest.prs["LB-006"].required_tests.filter((item) => !item.startsWith("a malicious earlier PATH pwsh.exe"));
assert.equal(hasExactLocalBridgeAgentRuntimeFacadeAmendment20260814(schema25MissingPathHijackTest), false);

assert.equal(hasExactG3UiFirstScrollbarAmendment20260814(schema24Contracts), true);
const schema23Contracts = normalizeG3UiFirstScrollbarAmendment20260814(schema24Contracts);
assert.equal(schema23Contracts.schema_version, 23);
assert.equal(Object.hasOwn(schema23Contracts.rules, "scrollbar_arrow_affordances_forbidden"), false);
assert.equal(Object.hasOwn(schema23Contracts.rules, "foreground_ui_ready_before_runtime_start_required"), false);
assert.equal(schema23Contracts.prs["LB-014"].required_artifacts.includes("foreground configured launch automatic runtime start"), true);
assert.equal(schema23Contracts.prs["LB-015"].required_artifacts.includes("arrowless rounded-scroll presentation"), false);

const schema24ArrowRegression = structuredClone(schema24Contracts);
schema24ArrowRegression.rules.scrollbar_arrow_affordances_forbidden = false;
assert.equal(hasExactG3UiFirstScrollbarAmendment20260814(schema24ArrowRegression), false);
const schema24MissingArrowTest = structuredClone(schema24Contracts);
schema24MissingArrowTest.prs["LB-015"].required_tests = schema24MissingArrowTest.prs["LB-015"].required_tests.filter((item) => !item.startsWith("vertical scrollbars on Settings"));
assert.equal(hasExactG3UiFirstScrollbarAmendment20260814(schema24MissingArrowTest), false);
const schema24PrematureServiceStart = structuredClone(schema24Contracts);
schema24PrematureServiceStart.prs["LB-014"].required_tests = schema24PrematureServiceStart.prs["LB-014"].required_tests.map((item) => item.startsWith("foreground UI is created shown and interactive") ? "foreground runtime may start before the UI becomes interactive" : item);
assert.equal(hasExactG3UiFirstScrollbarAmendment20260814(schema24PrematureServiceStart), false);
const schema24MissingReadyIdempotency = structuredClone(schema24Contracts);
schema24MissingReadyIdempotency.prs["LB-014"].required_tests = schema24MissingReadyIdempotency.prs["LB-014"].required_tests.filter((item) => !item.startsWith("duplicate UI-ready delivery"));
assert.equal(hasExactG3UiFirstScrollbarAmendment20260814(schema24MissingReadyIdempotency), false);
const schema24MissingBackgroundException = structuredClone(schema24Contracts);
schema24MissingBackgroundException.prs["LB-014"].required_tests = schema24MissingBackgroundException.prs["LB-014"].required_tests.filter((item) => !item.startsWith("--background startup does not wait"));
assert.equal(hasExactG3UiFirstScrollbarAmendment20260814(schema24MissingBackgroundException), false);

assert.equal(hasExactG3ManualReviewRound2_20260814(schema23Contracts), true);
const schema22Contracts = normalizeG3ManualReviewRound2_20260814(schema23Contracts);
assert.equal(schema22Contracts.schema_version, 22);
assert.equal(schema22Contracts.rules.dashboard_current_task_status, "single_current_or_last_timing_projection");
assert.deepEqual(schema22Contracts.rules.onboarding_window_default_inner_size, [900, 620]);
assert.equal(Object.hasOwn(schema22Contracts.rules, "task_backend_wakeup_delivery_required"), false);
assert.equal(Object.hasOwn(schema22Contracts.rules, "settings_runtime_api_key_clear_action_required"), false);
assert.equal(Object.hasOwn(schema22Contracts.rules, "scrollable_rounded_surface_preserves_outer_corners"), false);
assert.equal(schema22Contracts.prs["LB-015"].writable_paths.includes("src-tauri/src/tray/**"), false);
assert.equal(schema22Contracts.prs["LB-016"].required_artifacts.includes("fixed 900x620 non-resizable non-maximizable main window"), true);

const schema23WeakWakeup = structuredClone(schema23Contracts);
schema23WeakWakeup.rules.task_backend_wakeup_delivery_required = false;
assert.equal(hasExactG3ManualReviewRound2_20260814(schema23WeakWakeup), false);
const schema23ShortVisibility = structuredClone(schema23Contracts);
schema23ShortVisibility.rules.task_minimum_visible_duration_ms = 499;
assert.equal(hasExactG3ManualReviewRound2_20260814(schema23ShortVisibility), false);
const schema23OldWindow = structuredClone(schema23Contracts);
schema23OldWindow.rules.onboarding_window_default_inner_size = [900, 620];
assert.equal(hasExactG3ManualReviewRound2_20260814(schema23OldWindow), false);
const schema23WeakButtonGeometry = structuredClone(schema23Contracts);
schema23WeakButtonGeometry.rules.onboarding_two_line_button_min_rendered_height_multiplier = 1;
assert.equal(hasExactG3ManualReviewRound2_20260814(schema23WeakButtonGeometry), false);
const schema23RemovedSavedKeyMask = structuredClone(schema23Contracts);
delete schema23RemovedSavedKeyMask.rules.onboarding_saved_runtime_key_mask_matches_saved_secret_length;
assert.equal(hasExactG3ManualReviewRound2_20260814(schema23RemovedSavedKeyMask), false);
const schema23RemovedKeyClear = structuredClone(schema23Contracts);
delete schema23RemovedKeyClear.rules.settings_runtime_api_key_clear_action_required;
assert.equal(hasExactG3ManualReviewRound2_20260814(schema23RemovedKeyClear), false);
const schema23RemovedRoundedScroll = structuredClone(schema23Contracts);
delete schema23RemovedRoundedScroll.rules.scrollable_rounded_surface_preserves_outer_corners;
assert.equal(hasExactG3ManualReviewRound2_20260814(schema23RemovedRoundedScroll), false);

assert.equal(hasExactG3ManualPathExecutionCorrection20260814(schema22Contracts), true);
const schema21Contracts = normalizeG3ManualPathExecutionCorrection20260814(schema22Contracts);
assert.equal(schema21Contracts.schema_version, 21);
assert.equal(schema21Contracts.rules.workspace_internal_resolved_verbatim_path_allowed, true);
assert.equal(Object.hasOwn(schema21Contracts.rules, "workspace_command_cwd_verbatim_prefix_forbidden"), false);
assert.equal(schema21Contracts.prs["LB-015"].writable_paths.includes("src-tauri/src/runtime/**"), false);
const schema22BroadVerbatimRegression = structuredClone(schema22Contracts);
schema22BroadVerbatimRegression.rules.workspace_internal_resolved_verbatim_path_allowed = true;
assert.equal(hasExactG3ManualPathExecutionCorrection20260814(schema22BroadVerbatimRegression), false);
for (const rule of [
  "workspace_command_cwd_verbatim_prefix_forbidden",
  "workspace_tool_invocation_verbatim_prefix_forbidden",
  "workspace_sidecar_current_dir_verbatim_prefix_forbidden",
  "workspace_execution_path_derived_from_validated_identity_required",
]) {
  const weakened = structuredClone(schema22Contracts);
  delete weakened.rules[rule];
  assert.equal(hasExactG3ManualPathExecutionCorrection20260814(weakened), false);
}
const schema22MissingRuntimeScope = structuredClone(schema22Contracts);
schema22MissingRuntimeScope.prs["LB-015"].writable_paths = schema22MissingRuntimeScope.prs["LB-015"].writable_paths.filter((item) => item !== "src-tauri/src/runtime/**");
assert.equal(hasExactG3ManualPathExecutionCorrection20260814(schema22MissingRuntimeScope), false);
assert.equal(hasExactG3ManualSupplement20260814(schema21Contracts), true);
const generation2Contracts = normalizeG3ManualSupplement20260814(schema21Contracts);
assert.equal(hasExactG3HumanReviewGeneration2Amendment(generation2Contracts), true);
assert.equal(generation2Contracts.schema_version, 20);
assert.equal(generation2Contracts.rules.permission_mode_post_onboarding_edit_surface, "settings_only");
assert.equal(Object.hasOwn(generation2Contracts.rules, "permission_mode_post_onboarding_edit_surfaces"), false);
const rolledBackReopenedOnboardingPermission = structuredClone(schema21Contracts);
delete rolledBackReopenedOnboardingPermission.rules.permission_mode_post_onboarding_edit_surfaces;
delete rolledBackReopenedOnboardingPermission.rules.reopened_onboarding_screen_3_permission_edit_allowed;
rolledBackReopenedOnboardingPermission.rules.permission_mode_post_onboarding_edit_surface = "settings_only";
assert.equal(hasExactG3ManualSupplement20260814(rolledBackReopenedOnboardingPermission), false);
const weakenedStartingReconnect = structuredClone(schema21Contracts);
weakenedStartingReconnect.prs["LB-015"].required_tests = weakenedStartingReconnect.prs["LB-015"].required_tests.map((item) => item.startsWith("Settings save validates changed fields") ? "Settings save validates changed fields then securely writes and reconnects only when runtime is active" : item);
assert.equal(hasExactG3ManualSupplement20260814(weakenedStartingReconnect), false);
const removedTaskTiming = structuredClone(schema21Contracts);
delete removedTaskTiming.rules.task_active_elapsed_required;
assert.equal(hasExactG3ManualSupplement20260814(removedTaskTiming), false);
const removedPathPresentation = structuredClone(schema21Contracts);
delete removedPathPresentation.rules.workspace_user_facing_verbatim_prefix_forbidden;
assert.equal(hasExactG3ManualSupplement20260814(removedPathPresentation), false);
const removedButtonAlignment = structuredClone(schema21Contracts);
delete removedButtonAlignment.rules.settings_replace_buttons_same_action_column_required;
assert.equal(hasExactG3ManualSupplement20260814(removedButtonAlignment), false);
const removedServiceControls = structuredClone(schema21Contracts);
delete removedServiceControls.rules.dashboard_service_controls;
assert.equal(hasExactG3ManualSupplement20260814(removedServiceControls), false);
const normalizedGeneration2 = normalizeG3HumanReviewGeneration2Amendment(generation2Contracts.prs);
assert.equal(normalizedGeneration2["LB-013"].writable_paths.includes("src-tauri/src/settings/**"), false);
assert.equal(normalizedGeneration2["LB-015"].required_tests.includes("only enable/disable admin controls"), true);
assert.equal(normalizedGeneration2["LB-016"].required_tests.includes("UAC only from explicit user action"), true);
assert.equal(normalizedGeneration2["LB-017"].required_artifacts.includes("minimal diagnostics"), true);
const historicalBaselineUnaffected = normalizeG3HumanReviewGeneration2Amendment(beforePrContracts.prs);
assert.deepEqual(historicalBaselineUnaffected, beforePrContracts.prs);
const weakenedGeneration2 = structuredClone(generation2Contracts);
weakenedGeneration2.rules.frontend_is_typed_projection_only = false;
assert.equal(hasExactG3HumanReviewGeneration2Amendment(weakenedGeneration2), false);
const reintroducedEnableButton = structuredClone(generation2Contracts);
reintroducedEnableButton.prs["LB-015"].required_tests = reintroducedEnableButton.prs["LB-015"].required_tests.filter((item) => !item.startsWith("Settings has no separate 启用管理员权限 button"));
assert.equal(hasExactG3HumanReviewGeneration2Amendment(reintroducedEnableButton), false);
const reintroducedDashboardPermissionMode = structuredClone(generation2Contracts);
reintroducedDashboardPermissionMode.rules.dashboard_permission_mode_controls_forbidden = false;
assert.equal(hasExactG3HumanReviewGeneration2Amendment(reintroducedDashboardPermissionMode), false);
const removedDashboardNoModeTest = structuredClone(generation2Contracts);
removedDashboardNoModeTest.prs["LB-015"].required_tests = removedDashboardNoModeTest.prs["LB-015"].required_tests.filter((item) => !item.startsWith("Dashboard renders no 权限模式 row"));
assert.equal(hasExactG3HumanReviewGeneration2Amendment(removedDashboardNoModeTest), false);
const rolledBackDashboardAmber = structuredClone(generation2Contracts);
rolledBackDashboardAmber.prs["LB-016"].required_tests = rolledBackDashboardAmber.prs["LB-016"].required_tests.map((item) => item.includes("amber logical styling in onboarding and Settings") ? item.replace("onboarding and Settings", "onboarding and Dashboard") : item);
assert.equal(hasExactG3HumanReviewGeneration2Amendment(rolledBackDashboardAmber), false);
assert.doesNotMatch(validatePreG4GateAuthorization(generation2Contracts, {
  ...ratifiedGit,
  commitExists: (commit) => commit === "a19297a77688aebe1f3d807f28da6b3fad1dcbcb" || ratifiedGit.commitExists(commit),
  isAncestor: (commit) => commit === "a19297a77688aebe1f3d807f28da6b3fad1dcbcb" || ratifiedGit.isAncestor(commit),
  jsonAt: (revision, path) => revision === "a19297a77688aebe1f3d807f28da6b3fad1dcbcb" && path === "PR_CONTRACTS.json"
    ? { schema_version: 19, rules: Object.fromEntries(Object.entries(generation2Contracts.rules).filter(([key]) => !["dashboard_permission_mode_row_forbidden", "dashboard_permission_mode_controls_forbidden", "dashboard_permission_mode_change_forbidden", "dashboard_permission_mode_uac_trigger_forbidden", "permission_mode_edit_surfaces", "permission_mode_post_onboarding_edit_surface", "dashboard_admin_privilege_status_read_only", "visible_admin_mode_selection_requests_uac", "separate_enable_admin_button_forbidden", "leaving_admin_mode_disables_broker", "background_admin_preference_auto_uac_forbidden", "foreground_configured_launch_auto_starts_runtime", "foreground_runtime_start_must_not_block_ui", "frontend_is_typed_projection_only", "frontend_runtime_readiness_polling_state_machine_forbidden", "blocking_backend_work_on_ui_thread_forbidden", "ui_responsiveness_under_slow_backend_required", "dashboard_idle_text", "dashboard_idle_row_must_remain_visible", "dashboard_frontend_synthesized_task_state_forbidden", "settings_connection_field_labels", "runtime_api_key_user_facing_label", "runtime_api_key_label_translation_forbidden", "settings_test_connection_button_forbidden", "settings_connection_partial_update_required", "settings_save_controlled_reconnect_on_effective_connection_change", "settings_sections", "settings_general_controls", "settings_footer_actions", "close_window_continue_running_setting_required", "diagnostics_sections", "diagnostics_actions", "diagnostics_engineering_generation_details_forbidden", "onboarding_screen_3_permission_min_height_px", "dashboard_native_windows_folder_picker_required", "cloudflared_final_bundle_forbidden", "cloudflare_managed_tunnel_runtime_forbidden", "cloudflare_historical_compatibility_evidence_may_remain_non_executable"].includes(key))), prs: normalizedGeneration2 }
    : ratifiedGit.jsonAt(revision, path),
}, expected, ratification).join("|"), /human-review-contract-amendment-drift/);

const base = {
  execution: { current_group: "G3", current_pr: null },
  prs: [{ id: "LB-017", status: "PASS" }, { id: "LB-018", status: "BLOCKED" }],
  groups: [
    { id: "G3", prs: ["LB-013", "LB-014", "LB-015", "LB-016", "LB-017"], status: "PASS", review_status: "PASS", review_generation: 1,
      review_provenance: { generation: 1, kind: "independent_adversarial", commit: reviewCommit }, human_review_status: "REQUIRED", human_review_generation: 1 },
    { id: "G4", status: "BLOCKED", review_status: "BLOCKED", review_generation: 0 },
  ],
};
const reviewAfter = structuredClone(base);
const humanBefore = structuredClone(base);
const humanAfterPass = structuredClone(base);
humanAfterPass.execution = { current_group: "G4", current_pr: "LB-018" };
humanAfterPass.prs.find((p) => p.id === "LB-018").status = "READY";
humanAfterPass.groups.find((g) => g.id === "G3").human_review_status = "PASS";
humanAfterPass.groups.find((g) => g.id === "G4").status = "READY";
const git = {
  isAncestor: () => true,
  commitPaths: () => ["PR_INDEX.json", "PROJECT_STATE.json"],
  jsonAt: (revision, path) => {
    if (path !== "PR_INDEX.json") return null;
    if (revision === reviewCommit) return reviewAfter;
    if (revision === `${humanDecisionCommit}^`) return humanBefore;
    if (revision === humanDecisionCommit) return humanAfterPass;
    return null;
  },
};
assert.deepEqual(validateG4HumanGate(base, git), []);
const bypass = structuredClone(base);
bypass.groups.find((g) => g.id === "G4").status = "READY";
bypass.prs.find((p) => p.id === "LB-018").status = "READY";
assert.match(validateG4HumanGate(bypass, git).join("|"), /g4-unlocked-without-double-pass/);

const passed = structuredClone(humanAfterPass);
passed.groups.find((g) => g.id === "G3").human_review_provenance = {
  generation: 1,
  kind: "human_manual_test_review",
  commit: humanDecisionCommit,
  claims_treated_as_untrusted_evidence: true,
  pre_authorization_inventory_complete: true,
  evidence_records: [{ id: "MANUAL-001", source: "user_manual_test", evidence_ref: "conversation:test-001", verification: "reviewer cross-checked observed behavior", reviewer_disposition: "accepted_after_review" }],
  executor_pre_authorizations: [{ authorization_id: "PREAUTH-001", scope: "manual smoke command", actions: ["launch application"], evidence_ref: "conversation:preauth-001", recorded_by: "execution-agent", user_audit_status: "PASS" }],
};
assert.deepEqual(validateG4HumanGate(passed, git), []);
const unaudited = structuredClone(passed);
unaudited.groups.find((g) => g.id === "G3").human_review_provenance.executor_pre_authorizations[0].user_audit_status = "PENDING";
assert.match(validateG4HumanGate(unaudited, git).join("|"), /executor-preauthorization-not-user-audited-pass/);
const trustedClaims = structuredClone(passed);
trustedClaims.groups.find((g) => g.id === "G3").human_review_provenance.claims_treated_as_untrusted_evidence = false;
assert.match(validateG4HumanGate(trustedClaims, git).join("|"), /human-review-trust-policy/);

console.log("G4_HUMAN_GATE_TEST=PASS ai_pass_keeps_g4_blocked=true human_pass_required=true claims_untrusted=true preauth_concrete_and_user_audited=true");
