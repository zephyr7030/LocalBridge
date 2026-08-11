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
const beforePrContracts = { prs: { "LB-018": { writable_paths: ["src-tauri/**"] } } };
const contracts = {
  ...beforePrContracts,
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
assert.deepEqual(validatePreG4GateAuthorization(contracts, authGit, expected), []);
const widenedContracts = structuredClone(contracts);
widenedContracts.prs["LB-018"].writable_paths.push("src/**");
assert.match(validatePreG4GateAuthorization(widenedContracts, authGit, expected).join("|"), /ordinary-pr-contract-drift/);
const badAuthGit = { ...authGit, commitPaths: (commit) => commit === implementationCommit ? [...expected.authorizedPaths, "src-tauri/src/lib.rs"] : authGit.commitPaths(commit) };
assert.match(validatePreG4GateAuthorization(contracts, badAuthGit, expected).join("|"), /implementation-child-scope/);

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
