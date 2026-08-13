import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { PRE_G4_GATE_AUTHORIZATION, hasExactG3HumanReviewAmendment, hasExactG3HumanReviewGeneration2Amendment, normalizeG3HumanReviewAmendment, normalizeG3HumanReviewGeneration2Amendment, validateG4HumanGate, validatePreG4GateAuthorization } from "./g4-human-gate.mjs";

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

const generation2Contracts = JSON.parse(readFileSync(new URL("../../PR_CONTRACTS.json", import.meta.url), "utf8"));
assert.equal(hasExactG3HumanReviewGeneration2Amendment(generation2Contracts), true);
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
reintroducedEnableButton.prs["LB-015"].required_tests = reintroducedEnableButton.prs["LB-015"].required_tests.filter((item) => !item.startsWith("no separate 启用管理员权限 button exists"));
assert.equal(hasExactG3HumanReviewGeneration2Amendment(reintroducedEnableButton), false);

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
