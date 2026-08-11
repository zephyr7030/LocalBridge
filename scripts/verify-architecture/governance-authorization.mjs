const EXACT_COMMIT = /^[0-9a-f]{40}$/;
const EXACT_ID = /^GOV-AUTH-\d{3}$/;

const normalizePaths = (values) => [...new Set(values ?? [])]
  .map((value) => value.replaceAll("\\", "/"))
  .sort();

function parseAnchorMessage(message) {
  const lines = String(message ?? "").split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  if (!lines.includes("GOV_AUTH_V1")) return null;
  const fields = new Map();
  const repeated = new Map();
  for (const line of lines) {
    const split = line.indexOf("=");
    if (split <= 0) continue;
    const key = line.slice(0, split);
    const value = line.slice(split + 1);
    if (!repeated.has(key)) repeated.set(key, []);
    repeated.get(key).push(value);
    fields.set(key, value);
  }
  return { fields, repeated };
}

export function validateGovernanceAuthorizations(contractsDoc, git) {
  const authorizations = contractsDoc?.rules?.governance_authorizations ?? [];
  if (!Array.isArray(authorizations)) return ["inventory-not-array"];
  const findings = [];
  const ids = new Set();

  for (const authorization of authorizations) {
    const id = authorization?.id ?? "missing";
    if (!EXACT_ID.test(id) || ids.has(id)) {
      findings.push(`${id}:invalid-or-duplicate-id`);
      continue;
    }
    ids.add(id);
    if (authorization.scheme !== "git_empty_anchor_v1") findings.push(`${id}:scheme`);
    if (!EXACT_COMMIT.test(authorization.anchor_commit ?? "")) {
      findings.push(`${id}:anchor-commit`);
      continue;
    }

    const anchor = authorization.anchor_commit;
    if (!git.isAncestor(anchor)) findings.push(`${id}:anchor-not-ancestor`);
    const anchorPaths = normalizePaths(git.commitPaths(anchor));
    if (anchorPaths.length !== 0) findings.push(`${id}:anchor-not-empty`);

    const parsed = parseAnchorMessage(git.commitMessage(anchor));
    if (!parsed) {
      findings.push(`${id}:anchor-message-format`);
      continue;
    }
    const { fields, repeated } = parsed;
    if (fields.get("authorization_id") !== id) findings.push(`${id}:anchor-id`);
    if (fields.get("scope") !== "single_immediate_child_only") findings.push(`${id}:scope`);
    if (fields.get("does_not_expand_pr_writable_paths") !== "true") findings.push(`${id}:expands-pr-scope`);

    const declaredRatified = [...(authorization.ratified_commits ?? [])].sort();
    const anchorRatified = [...(repeated.get("ratifies_commit") ?? [])].sort();
    if (declaredRatified.length === 0
      || declaredRatified.some((commit) => !EXACT_COMMIT.test(commit))
      || JSON.stringify(declaredRatified) !== JSON.stringify(anchorRatified)) {
      findings.push(`${id}:ratified-commits`);
    }
    for (const commit of declaredRatified) {
      if (!git.isAncestor(commit) || !git.commitExists(commit)) findings.push(`${id}:ratified-commit-missing:${commit}`);
    }

    const declaredPaths = normalizePaths(authorization.authorized_child_paths);
    const anchorPathsField = normalizePaths((fields.get("authorized_immediate_child_paths") ?? "").split(",").filter(Boolean));
    if (declaredPaths.length === 0 || JSON.stringify(declaredPaths) !== JSON.stringify(anchorPathsField)) {
      findings.push(`${id}:authorized-paths`);
    }

    const parent = git.jsonAt(`${anchor}^`, "PR_INDEX.json");
    const lb005 = parent?.prs?.find((candidate) => candidate.id === "LB-005");
    const g1 = parent?.groups?.find((candidate) => candidate.id === "G1");
    if (parent?.execution?.current_pr !== "LB-005"
      || lb005?.status !== "REWORK_REQUIRED"
      || g1?.status !== "REWORK_REQUIRED"
      || g1?.review_status !== "FAIL"
      || g1?.review_generation !== 4) {
      findings.push(`${id}:anchor-parent-state`);
    }

    const child = git.firstParentChild(anchor);
    if (!EXACT_COMMIT.test(child ?? "")) {
      findings.push(`${id}:missing-immediate-child`);
      continue;
    }
    const childPaths = normalizePaths(git.commitPaths(child));
    if (JSON.stringify(childPaths) !== JSON.stringify(declaredPaths)) findings.push(`${id}:child-path-mismatch`);
    if (git.firstParentParent(child) !== anchor) findings.push(`${id}:child-not-immediate`);
  }

  return findings;
}
