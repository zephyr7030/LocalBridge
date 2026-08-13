import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { PRE_G4_GATE_AUTHORIZATION, validateG4HumanGate, validatePreG4GateAuthorization } from "./g4-human-gate.mjs";

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
);
authorizedLb016Rework.prs["LB-016"].required_tests = authorizedLb016Rework.prs["LB-016"].required_tests
  .map((item) => item === "screen 6 success message is 设置完成，尝试在 ChatGPT 中选择刚刚添加的连接器吧！"
    ? "screen 6 success message is 配置完成，在插件中选择刚刚添加的Local Bridge试试吧"
    : item);
authorizedLb016Rework.prs["LB-016"].required_tests.push(
  "screens 4 and 5 use Local Bridge as the user-facing connector term",
  "main window is fixed to 900x620 with minimum and maximum 900x620 resizable false and maximizable false",
  "native window decorations are disabled and exactly one edge-to-edge custom chrome provides drag minimize and close without maximize or double frame",
);
assert.deepEqual(validatePreG4GateAuthorization(authorizedLb016Rework, ratifiedGit, expected, ratification), []);
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
