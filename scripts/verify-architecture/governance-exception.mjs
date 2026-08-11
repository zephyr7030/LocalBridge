const EXACT_COMMIT = /^[0-9a-f]{40}$/;
const EXACT_ID = /^GOV-EX-\d{3}$/;

const normalizedSet = (values) => [...new Set(values ?? [])]
  .map((value) => value.replaceAll("\\", "/"))
  .sort();

export function validateOneTimeGovernanceExceptions(contractsDoc, git) {
  const exceptions = contractsDoc?.rules?.one_time_governance_exceptions ?? [];
  if (!Array.isArray(exceptions)) return ["inventory-not-array"];

  const findings = [];
  const ids = new Set();
  for (const exception of exceptions) {
    const id = exception?.id ?? "missing";
    if (!EXACT_ID.test(id) || ids.has(id)) {
      findings.push(`${id}:invalid-or-duplicate-id`);
      continue;
    }
    ids.add(id);

    if (exception.authorization !== "explicit_user_instruction") findings.push(`${id}:authorization`);
    if (exception.scope !== "historical_commit_only") findings.push(`${id}:scope`);
    if (exception.consumed !== true) findings.push(`${id}:not-consumed`);
    if (exception.does_not_expand_pr_writable_paths !== true) findings.push(`${id}:expands-pr-scope`);
    if (!EXACT_COMMIT.test(exception.commit ?? "")) {
      findings.push(`${id}:commit`);
      continue;
    }
    if (!/^LB-\d{3}$/.test(exception.parent_current_pr ?? "")) findings.push(`${id}:parent-current-pr`);
    if (exception.parent_pr_status !== "REWORK_REQUIRED") findings.push(`${id}:parent-pr-status`);

    const declaredPaths = normalizedSet(exception.allowed_paths);
    if (declaredPaths.length === 0 || declaredPaths.length !== (exception.allowed_paths ?? []).length) {
      findings.push(`${id}:allowed-paths`);
    }

    if (!git.isAncestor(exception.commit)) findings.push(`${id}:not-ancestor`);
    const actualPaths = normalizedSet(git.commitPaths(exception.commit));
    if (actualPaths.length === 0 || JSON.stringify(actualPaths) !== JSON.stringify(declaredPaths)) {
      findings.push(`${id}:commit-path-mismatch`);
    }

    const parent = git.jsonAt(`${exception.commit}^`, "PR_INDEX.json");
    const parentPr = parent?.prs?.find((candidate) => candidate.id === exception.parent_current_pr);
    if (parent?.execution?.current_pr !== exception.parent_current_pr
      || parentPr?.status !== exception.parent_pr_status) {
      findings.push(`${id}:parent-context`);
    }
  }
  return findings;
}
