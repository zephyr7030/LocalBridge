import { createHash } from "node:crypto";

const EXACT_COMMIT = /^[0-9a-f]{40}$/;

export const REAL_GOVERNANCE_AUTHORIZATION = Object.freeze({
  id: "GOV-AUTH-REAL-001",
  scheme: "user_file_sha256_v1",
  evidencePath: "授权信息.txt",
  evidenceCommit: "fd7c288aff61be1d882ad172a8cf8b267cabded8",
  evidenceCanonicalSha256: "ef21f583e8a2ec1d005cf58e325dee4f0eb4a171c1b0ac06ec80576647fe7504",
  ratifiedCommit: "5bb1b27f94feb6d0efbd7bc264c76ce9d056d03f",
  delegatedCommit: "f2101eb1909785c6cefef48050b2498a54a8f54b",
  historicalExceptionId: "GOV-EX-001",
});

const normalizePaths = (values) => [...new Set(values ?? [])]
  .map((value) => value.replaceAll("\\", "/"))
  .sort();

const canonicalText = (value) => String(value ?? "").replace(/\r\n/g, "\n");
const canonicalSha256 = (value) => createHash("sha256").update(canonicalText(value), "utf8").digest("hex");
const governanceRepairPath = (path) => path === "PR_CONTRACTS.json" || path.startsWith("scripts/verify-architecture/");

function requiredEvidenceFragments(expected) {
  return [
    "我作为 LocalBridge 项目的最终治理授权者，现明确授予一次性治理授权。",
    "授权 ID：GOV-AUTH-REAL-001",
    expected.ratifiedCommit,
    "一次且仅一次的后续治理修复",
    "仅允许修改治理合同及治理验证器相关文件",
    "不得修改 LocalBridge 产品代码",
    "LB-005 credential 实现",
    "完成并提交对应治理修复后立即失效",
    "不得复用于任何未来 PR、未来治理修改或其他 scope violation",
  ];
}

function exactContractEntry(contractsDoc, expected) {
  const entries = contractsDoc?.rules?.governance_authorizations;
  if (!Array.isArray(entries)) return null;
  const entry = entries.find((candidate) => candidate?.id === expected.id);
  if (!entry) return null;
  const ratified = [...(entry.ratified_commits ?? [])].sort();
  if (entry.id !== expected.id
    || entry.scheme !== expected.scheme
    || entry.evidence_path !== expected.evidencePath
    || entry.evidence_commit !== expected.evidenceCommit
    || entry.evidence_canonical_sha256 !== expected.evidenceCanonicalSha256
    || entry.scope !== "single_governance_repair_commit"
    || entry.does_not_expand_pr_writable_paths !== true
    || JSON.stringify(ratified) !== JSON.stringify([expected.ratifiedCommit])) {
    return null;
  }
  return entry;
}

export function validateGovernanceAuthorizations(contractsDoc, git, expected = REAL_GOVERNANCE_AUTHORIZATION) {
  const findings = [];
  if (!exactContractEntry(contractsDoc, expected)) {
    return [`${expected.id}:contract-mismatch`];
  }

  const evidenceCommit = expected.evidenceCommit;
  if (!git.commitExists(evidenceCommit) || !git.isAncestor(evidenceCommit)) {
    findings.push(`${expected.id}:evidence-commit-missing`);
    return findings;
  }

  const evidencePaths = normalizePaths(git.commitPaths(evidenceCommit));
  if (JSON.stringify(evidencePaths) !== JSON.stringify([expected.evidencePath])) {
    findings.push(`${expected.id}:evidence-commit-scope`);
  }

  const committedEvidence = git.textAt(evidenceCommit, expected.evidencePath);
  if (canonicalSha256(committedEvidence) !== expected.evidenceCanonicalSha256) {
    findings.push(`${expected.id}:evidence-hash`);
  }
  const workingEvidence = git.workingText(expected.evidencePath);
  if (canonicalSha256(workingEvidence) !== expected.evidenceCanonicalSha256) {
    findings.push(`${expected.id}:working-evidence-drift`);
  }
  const normalizedEvidence = canonicalText(committedEvidence);
  for (const fragment of requiredEvidenceFragments(expected)) {
    if (!normalizedEvidence.includes(fragment)) findings.push(`${expected.id}:evidence-content`);
  }

  const parent = git.jsonAt(`${evidenceCommit}^`, "PR_INDEX.json");
  const lb005 = parent?.prs?.find((candidate) => candidate.id === "LB-005");
  const g1 = parent?.groups?.find((candidate) => candidate.id === "G1");
  if (parent?.execution?.current_pr !== "LB-005"
    || lb005?.status !== "REWORK_REQUIRED"
    || g1?.status !== "REWORK_REQUIRED"
    || g1?.review_status !== "FAIL"
    || g1?.review_generation !== 5) {
    findings.push(`${expected.id}:evidence-parent-state`);
  }

  if (!git.commitExists(expected.ratifiedCommit) || !git.isAncestor(expected.ratifiedCommit)) {
    findings.push(`${expected.id}:ratified-commit-missing`);
  } else {
    const ratifiedPaths = normalizePaths(git.commitPaths(expected.ratifiedCommit));
    if (ratifiedPaths.length === 0 || ratifiedPaths.some((path) => !governanceRepairPath(path))) {
      findings.push(`${expected.id}:ratified-commit-scope`);
    }
  }

  const historicalContract = git.jsonAt(expected.ratifiedCommit, "PR_CONTRACTS.json");
  const historicalException = historicalContract?.rules?.one_time_governance_exceptions
    ?.find((candidate) => candidate.id === expected.historicalExceptionId);
  const delegatedPaths = normalizePaths(historicalException?.allowed_paths);
  const actualDelegatedPaths = normalizePaths(git.commitPaths(expected.delegatedCommit));
  if (historicalException?.scope !== "historical_commit_only"
    || historicalException?.commit !== expected.delegatedCommit
    || historicalException?.consumed !== true
    || historicalException?.does_not_expand_pr_writable_paths !== true
    || delegatedPaths.length === 0
    || JSON.stringify(delegatedPaths) !== JSON.stringify(actualDelegatedPaths)) {
    findings.push(`${expected.id}:historical-delegation`);
  }

  const child = git.firstParentChild(evidenceCommit);
  if (!EXACT_COMMIT.test(child ?? "")) {
    findings.push(`${expected.id}:missing-repair-child`);
    return [...new Set(findings)];
  }
  if (git.firstParentParent(child) !== evidenceCommit) findings.push(`${expected.id}:repair-not-immediate`);
  const childPaths = normalizePaths(git.commitPaths(child));
  if (childPaths.length === 0 || childPaths.some((path) => !governanceRepairPath(path))) {
    findings.push(`${expected.id}:repair-child-scope`);
  }

  return [...new Set(findings)];
}
