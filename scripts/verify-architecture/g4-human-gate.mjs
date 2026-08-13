import { createHash } from "node:crypto";

const EXACT_COMMIT = /^[0-9a-f]{40}$/;
const EXACT_SHA256 = /^[0-9a-f]{64}$/;
const REVIEW_GOVERNANCE_PATHS = new Set(["PR_INDEX.json", "PROJECT_STATE.json"]);
const HUMAN_REVIEW_STATUSES = new Set(["BLOCKED", "REQUIRED", "PASS", "FAIL"]);
const PREAUTH_AUDIT_STATUSES = new Set(["PENDING", "PASS", "FAIL"]);
const EVIDENCE_DISPOSITIONS = new Set(["accepted_after_review", "independently_verified", "rejected", "unresolved"]);

export const PRE_G4_GATE_AUTHORIZATION = Object.freeze({
  id: "GOV-AUTH-G4-HUMAN-001",
  scheme: "user_file_sha256_v1",
  evidencePath: "governance/G4_HUMAN_GATE_AUTHORIZATION.txt",
  evidenceCommit: "15963a0ca227feac41e2bf522c5321562567c6f7",
  evidenceCanonicalSha256: "ebdc20eac990c77b6cd30a388792340bc0c1c0797187ba6ffcfac7e41cb61e7a",
  scope: "single_immediate_governance_commit",
  authorizedPaths: [
    "AGENTS.md",
    "ARCHITECTURE_RULES.json",
    "FINAL_REVIEW.json",
    "PR_CONTRACTS.json",
    "PR_INDEX.json",
    "PROJECT_STATE.json",
    "docs/06_PR_GROUPS_AND_EXECUTION.md",
    "docs/08_FINAL_REVIEW.md",
    "scripts/verify-architecture/g4-human-gate.mjs",
    "scripts/verify-architecture/g4-human-gate.test.mjs",
    "scripts/verify-architecture/governance-authorization.mjs",
    "scripts/verify-architecture/index.mjs",
    "skills/adversarial-review/SKILL.md",
    "templates/ADVERSARIAL_REVIEW_PROMPT.md",
    "templates/PR_EXECUTION_PROMPT.md",
  ],
});

export const G3_SIX_SCREEN_CONTRACT_RATIFICATION = Object.freeze({
  commit: "6084c69a03892a79ce066b2c532378dd2b2d3e44",
  paths: [
    "AGENTS.md",
    "PROJECT_STATE.json",
    "PR_CONTRACTS.json",
    "PR_INDEX.json",
    "START_HERE.md",
    "docs/01_PRODUCT_UX.md",
    "docs/04_UX_SPEC.md",
    "docs/06_PR_PLAN.md",
    "docs/07_ACCEPTANCE.md",
    "docs/07_ACCEPTANCE_MATRIX.md",
    "docs/08_FINAL_REVIEW.md",
    "skills/ui/SKILL.md",
  ],
});

const canonicalText = (value) => String(value ?? "").replace(/\r\n/g, "\n");
const canonicalSha256 = (value) => createHash("sha256").update(canonicalText(value), "utf8").digest("hex");
const normalizePaths = (values) => [...new Set(values ?? [])].map((value) => value.replaceAll("\\", "/")).sort();
const nonEmptyString = (value) => typeof value === "string" && value.trim().length > 0;

const LB016_AUTHORIZED_G3_REWORK_2026_08_13 = Object.freeze({
  oldSuccessTest: "screen 6 success message is 设置完成，尝试在 ChatGPT 中选择刚刚添加的连接器吧！",
  newSuccessTest: "screen 6 success message is 配置完成，在插件中选择刚刚添加的Local Bridge试试吧",
  addedArtifacts: [
    "viewport-responsive resizable onboarding layout",
    "native-window/WebView client-area resize synchronization",
  ],
  addedTests: [
    "screens 4 and 5 use Local Bridge as the user-facing connector term",
    "resizable onboarding window has a 720x500 minimum and wizard body adapts to viewport height without fixed card minimum height",
    "real Windows Tauri resize E2E cross-checks native client area against live WebView JS viewport for two native sizes plus maximize and proves Dashboard and onboarding reflow",
  ],
});

function normalizeAuthorizedSemanticCorrections(prs) {
  const normalized = structuredClone(prs ?? null);
  const lb016 = normalized?.["LB-016"];
  if (Array.isArray(lb016?.required_artifacts)) {
    lb016.required_artifacts = lb016.required_artifacts.filter((item) =>
      item !== "6-screen wizard"
      && !LB016_AUTHORIZED_G3_REWORK_2026_08_13.addedArtifacts.includes(item));
  }
  if (Array.isArray(lb016?.required_tests)) {
    lb016.required_tests = lb016.required_tests
      .filter((item) => !LB016_AUTHORIZED_G3_REWORK_2026_08_13.addedTests.includes(item))
      .map((item) => item === LB016_AUTHORIZED_G3_REWORK_2026_08_13.newSuccessTest
        ? LB016_AUTHORIZED_G3_REWORK_2026_08_13.oldSuccessTest
        : item);
  }
  return normalized;
}

function requiredAuthorizationEvidenceFragments() {
  return [
    "G3 全部 PR 完成并通过独立对抗性智能体审查后，不得直接开启 G4",
    "人工实测细审核",
    "有权质疑、拒绝直接采信、要求复核或独立验证执行智能体",
    "用户和执行智能体的陈述均属于待验证证据",
    "authorization_id",
    "user_audit_status",
    "不得扩大任何普通 PR writable_paths",
    "唯一的紧随治理实现提交",
  ];
}

function validateLaterContractRatification(git, ratification) {
  const findings = [];
  if (!ratification || !EXACT_COMMIT.test(ratification.commit ?? "")
    || !git.commitExists(ratification.commit) || !git.isAncestor(ratification.commit)) {
    return ["later-contract-ratification-commit-missing"];
  }
  if (JSON.stringify(normalizePaths(git.commitPaths(ratification.commit)))
    !== JSON.stringify(normalizePaths(ratification.paths))) {
    findings.push("later-contract-ratification-commit-scope");
  }
  const ratified = git.jsonAt(ratification.commit, "PR_CONTRACTS.json");
  const lb015 = ratified?.prs?.["LB-015"];
  const lb016 = ratified?.prs?.["LB-016"];
  const rules = ratified?.rules;
  if (ratified?.schema_version !== 18
    || rules?.onboarding_screen_count !== 6
    || rules?.onboarding_screen_7_forbidden !== true
    || rules?.ui_button_visible_affordance_required !== true
    || rules?.ui_white_on_white_ambiguous_button_forbidden !== true
    || rules?.ui_minimum_prompt_required !== true
    || !lb015?.required_artifacts?.includes("coherent visible button token system shared across product UI")
    || !lb016?.required_artifacts?.includes("six-screen onboarding flow")
    || !lb016?.required_tests?.includes("onboarding has exactly six screens")) {
    findings.push("later-contract-ratification-content");
  }
  return findings;
}

export function validatePreG4GateAuthorization(
  contractsDoc,
  git,
  expected = PRE_G4_GATE_AUTHORIZATION,
  laterRatification = G3_SIX_SCREEN_CONTRACT_RATIFICATION,
) {
  const findings = [];
  const entries = contractsDoc?.rules?.governance_authorizations;
  const entry = Array.isArray(entries) ? entries.find((candidate) => candidate?.id === expected.id) : null;
  if (!entry
    || entry.scheme !== expected.scheme
    || entry.evidence_path !== expected.evidencePath
    || entry.evidence_commit !== expected.evidenceCommit
    || entry.evidence_canonical_sha256 !== expected.evidenceCanonicalSha256
    || entry.scope !== expected.scope
    || entry.consumed !== true
    || entry.does_not_expand_pr_writable_paths !== true
    || JSON.stringify(normalizePaths(entry.authorized_paths)) !== JSON.stringify(normalizePaths(expected.authorizedPaths))) {
    return [`${expected.id}:contract-mismatch`];
  }
  if (!EXACT_COMMIT.test(expected.evidenceCommit) || !EXACT_SHA256.test(expected.evidenceCanonicalSha256)
    || !git.commitExists(expected.evidenceCommit) || !git.isAncestor(expected.evidenceCommit)) {
    findings.push(`${expected.id}:evidence-commit-missing`);
    return findings;
  }
  if (JSON.stringify(normalizePaths(git.commitPaths(expected.evidenceCommit))) !== JSON.stringify([expected.evidencePath])) {
    findings.push(`${expected.id}:evidence-commit-scope`);
  }
  const committedEvidence = git.textAt(expected.evidenceCommit, expected.evidencePath);
  if (canonicalSha256(committedEvidence) !== expected.evidenceCanonicalSha256) findings.push(`${expected.id}:evidence-hash`);
  if (canonicalSha256(git.workingText(expected.evidencePath)) !== expected.evidenceCanonicalSha256) findings.push(`${expected.id}:working-evidence-drift`);
  const normalizedEvidence = canonicalText(committedEvidence);
  for (const fragment of requiredAuthorizationEvidenceFragments()) {
    if (!normalizedEvidence.includes(fragment)) findings.push(`${expected.id}:evidence-content`);
  }
  const child = git.firstParentChild(expected.evidenceCommit);
  if (!EXACT_COMMIT.test(child ?? "") || !git.commitExists(child) || !git.isAncestor(child)) {
    findings.push(`${expected.id}:missing-implementation-child`);
    return [...new Set(findings)];
  }
  if (git.firstParentParent(child) !== expected.evidenceCommit) findings.push(`${expected.id}:implementation-not-immediate`);
  if (JSON.stringify(normalizePaths(git.commitPaths(child))) !== JSON.stringify(normalizePaths(expected.authorizedPaths))) {
    findings.push(`${expected.id}:implementation-child-scope`);
  }
  const beforeContracts = git.jsonAt(expected.evidenceCommit, "PR_CONTRACTS.json");
  let contractBaseline = beforeContracts;
  if (laterRatification) {
    const ratificationFindings = validateLaterContractRatification(git, laterRatification);
    for (const detail of ratificationFindings) findings.push(detail);
    if (ratificationFindings.length === 0) {
      contractBaseline = git.jsonAt(laterRatification.commit, "PR_CONTRACTS.json");
    }
  }
  if (JSON.stringify(normalizeAuthorizedSemanticCorrections(contractBaseline?.prs))
    !== JSON.stringify(normalizeAuthorizedSemanticCorrections(contractsDoc?.prs))) {
    findings.push(`${expected.id}:ordinary-pr-contract-drift`);
  }
  return [...new Set(findings)];
}

function validateEvidenceRecords(provenance, outcome, findings) {
  const records = provenance?.evidence_records;
  if (!Array.isArray(records) || records.length === 0) {
    findings.push("human-review-evidence-records");
    return;
  }
  let acceptedHumanManualEvidence = false;
  for (const record of records) {
    if (!nonEmptyString(record?.id)
      || !nonEmptyString(record?.source)
      || !nonEmptyString(record?.evidence_ref)
      || !nonEmptyString(record?.verification)
      || !EVIDENCE_DISPOSITIONS.has(record?.reviewer_disposition)) {
      findings.push("human-review-evidence-record");
      continue;
    }
    if (record.source === "user_manual_test"
      && new Set(["accepted_after_review", "independently_verified"]).has(record.reviewer_disposition)) {
      acceptedHumanManualEvidence = true;
    }
    if (outcome === "PASS" && record.reviewer_disposition === "unresolved") findings.push("human-review-unresolved-evidence");
  }
  if (outcome === "PASS" && !acceptedHumanManualEvidence) findings.push("human-review-missing-accepted-user-manual-test");
}

function validateExecutorPreAuthorizations(provenance, outcome, findings) {
  const records = provenance?.executor_pre_authorizations ?? [];
  if (!Array.isArray(records)) {
    findings.push("executor-preauthorization-records");
    return;
  }
  if (outcome === "PASS" && provenance?.pre_authorization_inventory_complete !== true) {
    findings.push("executor-preauthorization-inventory-not-complete");
  }
  for (const record of records) {
    if (!nonEmptyString(record?.authorization_id)
      || !nonEmptyString(record?.scope)
      || !Array.isArray(record?.actions)
      || record.actions.length === 0
      || record.actions.some((action) => !nonEmptyString(action))
      || !nonEmptyString(record?.evidence_ref)
      || !nonEmptyString(record?.recorded_by)
      || !PREAUTH_AUDIT_STATUSES.has(record?.user_audit_status)) {
      findings.push("executor-preauthorization-record");
      continue;
    }
    if (outcome === "PASS" && record.user_audit_status !== "PASS") findings.push("executor-preauthorization-not-user-audited-pass");
  }
}

function validateHumanReviewProvenance(progress, g3, git, findings) {
  const outcome = g3.human_review_status;
  if (!new Set(["PASS", "FAIL"]).has(outcome)) return;
  const provenance = g3.human_review_provenance;
  if (!provenance
    || provenance.generation !== g3.human_review_generation
    || provenance.kind !== "human_manual_test_review"
    || !EXACT_COMMIT.test(provenance.commit ?? "")) {
    findings.push("human-review-provenance");
    return;
  }
  if (provenance.claims_treated_as_untrusted_evidence !== true) findings.push("human-review-trust-policy");
  validateEvidenceRecords(provenance, outcome, findings);
  validateExecutorPreAuthorizations(provenance, outcome, findings);
  const paths = git.commitPaths(provenance.commit);
  if (!git.isAncestor(provenance.commit)
    || !paths
    || paths.length === 0
    || paths.some((path) => !REVIEW_GOVERNANCE_PATHS.has(path.replaceAll("\\", "/")))) {
    findings.push("human-review-decision-commit-scope");
  }
  const before = git.jsonAt(`${provenance.commit}^`, "PR_INDEX.json");
  const after = git.jsonAt(provenance.commit, "PR_INDEX.json");
  const beforeG3 = before?.groups?.find((candidate) => candidate.id === "G3");
  const afterG3 = after?.groups?.find((candidate) => candidate.id === "G3");
  const afterG4 = after?.groups?.find((candidate) => candidate.id === "G4");
  const afterLb18 = after?.prs?.find((candidate) => candidate.id === "LB-018");
  if (beforeG3?.status !== "PASS"
    || beforeG3?.review_status !== "PASS"
    || beforeG3?.human_review_status !== "REQUIRED"
    || beforeG3?.human_review_generation !== provenance.generation
    || afterG3?.human_review_status !== outcome) {
    findings.push("human-review-decision-transition");
  }
  if (outcome === "PASS") {
    if (afterG3?.status !== "PASS"
      || afterG3?.review_status !== "PASS"
      || afterG4?.status !== "READY"
      || afterLb18?.status !== "READY"
      || after?.execution?.current_group !== "G4"
      || after?.execution?.current_pr !== "LB-018") {
      findings.push("human-review-pass-unlock-transition");
    }
  } else {
    const reopenedId = after?.execution?.current_pr;
    const reopenedPr = after?.prs?.find((candidate) => candidate.id === reopenedId);
    if (afterG3?.status !== "REWORK_REQUIRED"
      || afterG3?.review_status !== "REQUIRED"
      || afterG4?.status !== "BLOCKED"
      || afterLb18?.status !== "BLOCKED"
      || after?.execution?.current_group !== "G3"
      || !g3.prs?.includes(reopenedId)
      || reopenedPr?.status !== "REWORK_REQUIRED") {
      findings.push("human-review-fail-reopen-transition");
    }
  }
}

export function validateG4HumanGate(progress, git) {
  const findings = [];
  const groups = progress?.groups ?? [];
  const prs = progress?.prs ?? [];
  const g3 = groups.find((candidate) => candidate.id === "G3");
  const g4 = groups.find((candidate) => candidate.id === "G4");
  const lb18 = prs.find((candidate) => candidate.id === "LB-018");
  if (!g3 || !g4 || !lb18) return findings;
  const humanStatus = g3.human_review_status ?? "BLOCKED";
  const humanGeneration = g3.human_review_generation ?? 0;
  if (!HUMAN_REVIEW_STATUSES.has(humanStatus) || !Number.isInteger(humanGeneration) || humanGeneration < 0) {
    findings.push("human-review-state");
    return findings;
  }
  const g4Unlocked = g4.status !== "BLOCKED" || lb18.status !== "BLOCKED";
  if (g4Unlocked && !(g3.status === "PASS" && g3.review_status === "PASS" && humanStatus === "PASS")) {
    findings.push("g4-unlocked-without-double-pass");
  }
  if (humanStatus === "BLOCKED" && g3.status === "PASS" && g3.review_status === "PASS") {
    findings.push("g3-pass-without-required-human-gate");
  }
  if (humanStatus === "REQUIRED") {
    if (g3.status !== "PASS"
      || g3.review_status !== "PASS"
      || g4.status !== "BLOCKED"
      || lb18.status !== "BLOCKED"
      || progress?.execution?.current_group !== "G3"
      || progress?.execution?.current_pr !== null) {
      findings.push("human-review-required-state");
    }
  }
  if (humanStatus === "PASS" && (g3.status !== "PASS" || g3.review_status !== "PASS")) findings.push("human-review-pass-without-g3-pass");
  if (humanStatus === "FAIL") {
    const reopenedId = progress?.execution?.current_pr;
    const reopenedPr = prs.find((candidate) => candidate.id === reopenedId);
    if (g3.status !== "REWORK_REQUIRED"
      || g3.review_status !== "REQUIRED"
      || g4.status !== "BLOCKED"
      || lb18.status !== "BLOCKED"
      || progress?.execution?.current_group !== "G3"
      || !g3.prs?.includes(reopenedId)
      || reopenedPr?.status !== "REWORK_REQUIRED") {
      findings.push("human-review-fail-state");
    }
  }
  if (g3.review_status === "PASS" && g3.review_provenance?.kind === "independent_adversarial" && EXACT_COMMIT.test(g3.review_provenance?.commit ?? "")) {
    const afterReview = git.jsonAt(g3.review_provenance.commit, "PR_INDEX.json");
    const afterG3 = afterReview?.groups?.find((candidate) => candidate.id === "G3");
    const afterG4 = afterReview?.groups?.find((candidate) => candidate.id === "G4");
    const afterLb18 = afterReview?.prs?.find((candidate) => candidate.id === "LB-018");
    if (afterG3?.human_review_status !== "REQUIRED"
      || !Number.isInteger(afterG3?.human_review_generation)
      || afterG3.human_review_generation < 1
      || afterG4?.status !== "BLOCKED"
      || afterLb18?.status !== "BLOCKED"
      || afterReview?.execution?.current_group !== "G3"
      || afterReview?.execution?.current_pr !== null) {
      findings.push("g3-adversarial-pass-transition-bypassed-human-gate");
    }
  }
  validateHumanReviewProvenance(progress, g3, git, findings);
  return [...new Set(findings)];
}
