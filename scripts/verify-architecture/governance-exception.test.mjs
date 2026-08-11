import assert from "node:assert/strict";
import { validateOneTimeGovernanceExceptions } from "./governance-exception.mjs";

const commit = "f2101eb1909785c6cefef48050b2498a54a8f54b";
const allowedPaths = [
  "AGENTS.md",
  "PR_INDEX.json",
  "START_HERE.md",
  "docs/06_PR_GROUPS_AND_EXECUTION.md",
  "scripts/verify-architecture/index.mjs",
];
const valid = {
  rules: {
    one_time_governance_exceptions: [{
      id: "GOV-EX-001",
      authorization: "explicit_user_instruction",
      scope: "historical_commit_only",
      commit,
      parent_current_pr: "LB-005",
      parent_pr_status: "REWORK_REQUIRED",
      allowed_paths: allowedPaths,
      consumed: true,
      does_not_expand_pr_writable_paths: true,
    }],
  },
};
const git = {
  isAncestor: (candidate) => candidate === commit,
  commitPaths: (candidate) => candidate === commit ? allowedPaths : [],
  jsonAt: (revision) => revision === `${commit}^`
    ? { execution: { current_pr: "LB-005" }, prs: [{ id: "LB-005", status: "REWORK_REQUIRED" }] }
    : null,
};

assert.deepEqual(validateOneTimeGovernanceExceptions(valid, git), []);

const clone = () => structuredClone(valid);
let bad = clone();
bad.rules.one_time_governance_exceptions[0].allowed_paths.push("src-tauri/src/credentials/windows.rs");
assert.match(validateOneTimeGovernanceExceptions(bad, git).join("|"), /commit-path-mismatch/);

bad = clone();
bad.rules.one_time_governance_exceptions[0].scope = "current_pr_and_future";
assert.match(validateOneTimeGovernanceExceptions(bad, git).join("|"), /scope/);

bad = clone();
bad.rules.one_time_governance_exceptions[0].consumed = false;
assert.match(validateOneTimeGovernanceExceptions(bad, git).join("|"), /not-consumed/);

bad = clone();
bad.rules.one_time_governance_exceptions[0].does_not_expand_pr_writable_paths = false;
assert.match(validateOneTimeGovernanceExceptions(bad, git).join("|"), /expands-pr-scope/);

const wrongParentGit = { ...git, jsonAt: () => ({ execution: { current_pr: "LB-004" }, prs: [] }) };
assert.match(validateOneTimeGovernanceExceptions(valid, wrongParentGit).join("|"), /parent-context/);

console.log("GOVERNANCE_EXCEPTION_TEST=PASS historical_only=true exact_commit=true exact_paths=true parent_context=true non_reusable=true");
