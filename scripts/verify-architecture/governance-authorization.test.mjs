import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  REAL_GOVERNANCE_AUTHORIZATION,
  validateGovernanceAuthorizations,
} from "./governance-authorization.mjs";

const evidenceCommit = "a".repeat(40);
const repairChild = "b".repeat(40);
const delegatedCommit = "c".repeat(40);
const ratifiedCommit = "d".repeat(40);
const delegatedPaths = [
  "AGENTS.md",
  "PR_INDEX.json",
  "START_HERE.md",
  "docs/06_PR_GROUPS_AND_EXECUTION.md",
  "scripts/verify-architecture/index.mjs",
];
const repairPaths = [
  "PR_CONTRACTS.json",
  "scripts/verify-architecture/governance-authorization.mjs",
  "scripts/verify-architecture/governance-authorization.test.mjs",
  "scripts/verify-architecture/governance-exception.mjs",
  "scripts/verify-architecture/governance-exception.test.mjs",
  "scripts/verify-architecture/index.mjs",
];
const evidence = [
  "我作为 LocalBridge 项目的最终治理授权者，现明确授予一次性治理授权。",
  "授权 ID：GOV-AUTH-REAL-001",
  `我明确追认以下历史治理提交： ${ratifiedCommit}`,
  "同时授权开发智能体进行一次且仅一次的后续治理修复。",
  "该后续治理修复仅允许修改治理合同及治理验证器相关文件，不得修改 LocalBridge 产品代码、LB-005 credential 实现。",
  "本授权仅用于解决上述既有治理问题，完成并提交对应治理修复后立即失效，不得复用于任何未来 PR、未来治理修改或其他 scope violation。",
].join("\n");
const expected = {
  ...REAL_GOVERNANCE_AUTHORIZATION,
  evidenceCommit,
  evidenceCanonicalSha256: createHash("sha256").update(evidence, "utf8").digest("hex"),
  ratifiedCommit,
  delegatedCommit,
};
const contracts = {
  rules: {
    governance_authorizations: [{
      id: expected.id,
      scheme: expected.scheme,
      evidence_path: expected.evidencePath,
      evidence_commit: expected.evidenceCommit,
      evidence_canonical_sha256: expected.evidenceCanonicalSha256,
      ratified_commits: [expected.ratifiedCommit],
      scope: "single_governance_repair_commit",
      does_not_expand_pr_writable_paths: true,
    }],
  },
};
const parentState = {
  execution: { current_pr: "LB-005" },
  prs: [{ id: "LB-005", status: "REWORK_REQUIRED" }],
  groups: [{ id: "G1", status: "REWORK_REQUIRED", review_status: "FAIL", review_generation: 5 }],
};
const historicalContract = {
  rules: {
    one_time_governance_exceptions: [{
      id: expected.historicalExceptionId,
      scope: "historical_commit_only",
      commit: expected.delegatedCommit,
      allowed_paths: delegatedPaths,
      consumed: true,
      does_not_expand_pr_writable_paths: true,
    }],
  },
};
const git = {
  isAncestor: (commit) => [evidenceCommit, ratifiedCommit, delegatedCommit].includes(commit),
  commitExists: (commit) => [evidenceCommit, repairChild, ratifiedCommit, delegatedCommit].includes(commit),
  commitPaths: (commit) => {
    if (commit === evidenceCommit) return [expected.evidencePath];
    if (commit === ratifiedCommit) return ["PR_CONTRACTS.json", "scripts/verify-architecture/index.mjs"];
    if (commit === delegatedCommit) return delegatedPaths;
    if (commit === repairChild) return repairPaths;
    return [];
  },
  textAt: (commit, path) => commit === evidenceCommit && path === expected.evidencePath ? evidence : null,
  workingText: (path) => path === expected.evidencePath ? evidence : null,
  jsonAt: (revision, path) => {
    if (revision === `${evidenceCommit}^` && path === "PR_INDEX.json") return parentState;
    if (revision === ratifiedCommit && path === "PR_CONTRACTS.json") return historicalContract;
    return null;
  },
  firstParentChild: (commit) => commit === evidenceCommit ? repairChild : null,
  firstParentParent: (commit) => commit === repairChild ? evidenceCommit : null,
};

assert.deepEqual(validateGovernanceAuthorizations(contracts, git, expected), []);

let bad = structuredClone(contracts);
bad.rules.governance_authorizations[0].evidence_commit = "e".repeat(40);
assert.match(validateGovernanceAuthorizations(bad, git, expected).join("|"), /contract-mismatch/);

let badGit = { ...git, commitPaths: (commit) => commit === evidenceCommit ? [expected.evidencePath, "PR_CONTRACTS.json"] : git.commitPaths(commit) };
assert.match(validateGovernanceAuthorizations(contracts, badGit, expected).join("|"), /evidence-commit-scope/);

badGit = { ...git, workingText: () => null };
assert.deepEqual(validateGovernanceAuthorizations(contracts, badGit, expected), []);

badGit = { ...git, jsonAt: (revision, path) => revision === `${evidenceCommit}^` && path === "PR_INDEX.json"
  ? { ...parentState, groups: [{ id: "G1", status: "REWORK_REQUIRED", review_status: "FAIL", review_generation: 4 }] }
  : git.jsonAt(revision, path) };
assert.match(validateGovernanceAuthorizations(contracts, badGit, expected).join("|"), /evidence-parent-state/);

badGit = { ...git, commitPaths: (commit) => commit === repairChild ? [...repairPaths, "src-tauri/src/credentials/windows.rs"] : git.commitPaths(commit) };
assert.match(validateGovernanceAuthorizations(contracts, badGit, expected).join("|"), /repair-child-scope/);

badGit = { ...git, jsonAt: (revision, path) => revision === ratifiedCommit && path === "PR_CONTRACTS.json" ? { rules: {} } : git.jsonAt(revision, path) };
assert.match(validateGovernanceAuthorizations(contracts, badGit, expected).join("|"), /historical-delegation/);

console.log("GOVERNANCE_AUTHORIZATION_TEST=PASS historical_commit_evidence=true working_copy_required=false exact_hash=true generation5_parent=true ratified_5bb=true delegated_f210=true single_repair_child=true product_code_denied=true");
