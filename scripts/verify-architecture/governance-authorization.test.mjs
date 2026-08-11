import assert from "node:assert/strict";
import { validateGovernanceAuthorizations } from "./governance-authorization.mjs";

const anchor = "a".repeat(40);
const child = "b".repeat(40);
const f210 = "c".repeat(40);
const b5 = "d".repeat(40);
const paths = [
  "PR_CONTRACTS.json",
  "scripts/verify-architecture/governance-authorization.mjs",
  "scripts/verify-architecture/governance-authorization.test.mjs",
  "scripts/verify-architecture/index.mjs",
];
const message = [
  "chore(governance): anchor user-authorized G1 repair",
  "GOV_AUTH_V1",
  "authorization_id=GOV-AUTH-001",
  `ratifies_commit=${b5}`,
  `ratifies_commit=${f210}`,
  `authorized_immediate_child_paths=${paths.join(",")}`,
  "scope=single_immediate_child_only",
  "does_not_expand_pr_writable_paths=true",
].join("\n");
const contracts = {
  rules: {
    governance_authorizations: [{
      id: "GOV-AUTH-001",
      scheme: "git_empty_anchor_v1",
      anchor_commit: anchor,
      ratified_commits: [f210, b5],
      authorized_child_paths: paths,
    }],
  },
};
const parentState = {
  execution: { current_pr: "LB-005" },
  prs: [{ id: "LB-005", status: "REWORK_REQUIRED" }],
  groups: [{ id: "G1", status: "REWORK_REQUIRED", review_status: "FAIL", review_generation: 4 }],
};
const git = {
  isAncestor: (commit) => [anchor, f210, b5].includes(commit),
  commitExists: (commit) => [anchor, child, f210, b5].includes(commit),
  commitPaths: (commit) => commit === anchor ? [] : commit === child ? paths : ["historical"],
  commitMessage: (commit) => commit === anchor ? message : "",
  jsonAt: (revision) => revision === `${anchor}^` ? parentState : null,
  firstParentChild: (commit) => commit === anchor ? child : null,
  firstParentParent: (commit) => commit === child ? anchor : null,
};

assert.deepEqual(validateGovernanceAuthorizations(contracts, git), []);
const clone = () => structuredClone(contracts);

let badGit = { ...git, commitPaths: (commit) => commit === anchor ? ["PR_CONTRACTS.json"] : git.commitPaths(commit) };
assert.match(validateGovernanceAuthorizations(contracts, badGit).join("|"), /anchor-not-empty/);

badGit = { ...git, jsonAt: () => ({ execution: { current_pr: "LB-005" }, prs: [{ id: "LB-005", status: "PASS" }], groups: parentState.groups }) };
assert.match(validateGovernanceAuthorizations(contracts, badGit).join("|"), /anchor-parent-state/);

let bad = clone();
bad.rules.governance_authorizations[0].ratified_commits = [f210];
assert.match(validateGovernanceAuthorizations(bad, git).join("|"), /ratified-commits/);

bad = clone();
bad.rules.governance_authorizations[0].authorized_child_paths.push("src-tauri/src/credentials/windows.rs");
assert.match(validateGovernanceAuthorizations(bad, git).join("|"), /authorized-paths|child-path-mismatch/);

badGit = { ...git, firstParentChild: () => null };
assert.match(validateGovernanceAuthorizations(contracts, badGit).join("|"), /missing-immediate-child/);

badGit = { ...git, commitMessage: () => "authorization_id=GOV-AUTH-001" };
assert.match(validateGovernanceAuthorizations(contracts, badGit).join("|"), /anchor-message-format/);

console.log("GOVERNANCE_AUTHORIZATION_TEST=PASS empty_anchor=true parent_generation4_fail=true exact_ratification=true immediate_child_only=true exact_paths=true malformed_rejected=true");
