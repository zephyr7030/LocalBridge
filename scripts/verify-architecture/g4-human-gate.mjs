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
    "screen 4 create-custom-plugin persisted-information panel",
    "fixed 900x620 non-resizable non-maximizable main window",
    "single edge-to-edge custom window chrome with native decorations disabled",
    "full-page onboarding layout using the fixed custom-chrome content area",
  ],
  addedTests: [
    "screen 4 title is 创建自定义插件",
    "screen 4 ChatGPT plugin-settings action opens only https://chatgpt.com/plugins#settings/Plugins in the system default browser through a fixed Rust allowlist",
    "screen 4 shows exactly two concise information rows 名称 Tunnel ID and does not show 本地服务",
    "screen 4 名称 value is Local Bridge",
    "screen 4 Tunnel ID value reflects the current persisted saved value rather than an unsaved frontend-only value",
    "each of the two screen 4 information rows has its own copy action and successful copy shows green 已复制 for exactly 3 seconds before restoring without layout shift",
    "screen 3 permission mode buttons preserve clearly visible balanced content-to-border spacing in the real 900x620 render and grow safely for wrapped descriptive text; final visual PASS requires human inspection and cannot be inferred from CSS padding markers alone",
    "the selected project runtime MCP and OpenAI Tunnel are all ready before screen 4 becomes reachable so plugin creation is executable rather than premature guidance",
    "onboarding never defers its only runtime startup edge until screen 5 or screen 6",
    "screens 4 and 5 use Local Bridge as the user-facing connector term",
    "main window is fixed to 900x620 with minimum and maximum 900x620 resizable false and maximizable false",
    "native window decorations are disabled and exactly one edge-to-edge custom chrome provides drag minimize and close without maximize or double frame",
    "onboarding uses the full fixed client content area without a centered floating card modal shell or large empty surrounding canvas",
  ],
  replacedTests: [
    ["screen 4 developer-mode guidance is 在插件设置页面最底端，打开“开发者模式”", "screen 4 custom connector guidance is concise and foolproof"],
    ["screen 4 lower plugin action label is 打开插件管理页", "screen 4 custom connector setup button"],
    ["screen 4 plugin management action opens only https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins in the system default browser through a fixed Rust allowlist", "screen 4 opens only https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins in the system default browser"],
  ],
});

const G3_HUMAN_REVIEW_AMENDMENT_2026_08_13 = Object.freeze({
  schemaVersion: 19,
  lb015AddedArtifacts: [
    "blue #0071e3 standard product accent with amber administrator-mode exception",
    "shared typed service-status dot presentation for Dashboard and onboarding",
  ],
  lb015AddedTests: [
    "primary and ordinary selected controls use the blue #0071e3 accent rather than black",
    "administrator mode uses amber logical selection styling and is not overridden by ordinary blue selected styling",
    "Dashboard tunnel and coding service states render status dots using Ready green Starting amber Fault red Unknown gray semantics from the same typed status source used by onboarding",
    "Dashboard does not maintain an independent conflicting service-status color state",
  ],
  lb016ArtifactReplacement: ["five-screen onboarding flow", "six-screen onboarding flow"],
  lb016AddedTests: [
    "screen 4 ChatGPT plugin-settings action is placed in the left-side action flow",
    "screen 4 shows 打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件 beneath the plugin-settings action",
    "screen 4 lower plugin-management action is placed in the left-side action flow",
    "ordinary selected permission modes use the standard blue accent while administrator mode uses amber logical styling in onboarding and Dashboard",
    "screen 4 provides an explicit back action to screen 3 and a continue action to screen 5",
    "screens 2 3 4 and 5 each provide an explicit back path and save start or configuration failure never traps the user",
    "screen 5 status dots map Ready to green Starting to amber Fault to red and Unknown to gray using the shared typed service-status source",
    "screen 5 provides an explicit back action to screen 4",
    "primary and ordinary selected wizard controls use the blue #0071e3 accent rather than black",
  ],
  lb016ReplacedTests: [
    ["onboarding has exactly five screens", "onboarding has exactly six screens"],
    ["onboarding never defers its only runtime startup edge until screen 5", "onboarding never defers its only runtime startup edge until screen 5 or screen 6"],
    ["screen 5 contains only local runtime environment coding service and OpenAI Tunnel checks", "screen 6 contains only local runtime environment coding service and OpenAI Tunnel checks"],
    ["screen 5 confirm is disabled until all three checks are green", "screen 6 confirm is disabled until all three checks are green"],
    ["screen 5 success message is hidden until all three checks are green", "screen 6 success message is hidden until all three checks are green"],
    ["screen 5 success message is 配置完成，在插件中选择刚刚添加的Local Bridge试试吧", "screen 6 success message is 配置完成，在插件中选择刚刚添加的Local Bridge试试吧"],
    ["screen 5 does not auto-advance", "screen 6 does not auto-advance"],
    ["screen 5 confirm enters main UI after readiness", "screen 6 confirm enters main UI after readiness"],
    ["onboarding has no sixth screen", "onboarding has no seventh screen"],
    ["screen 4 uses Local Bridge as the user-facing connector term", "screens 4 and 5 use Local Bridge as the user-facing connector term"],
  ],
  lb016RemovedTests: [
    "screen 5 provides only the minimum connector confirmation/use guidance and does not pretend to detect ChatGPT state",
    "if a connector endpoint is displayed or copied it comes from a typed Rust projection backed by verified tunnel or control-plane metadata",
    "frontend never derives a connector endpoint from Tunnel ID or fabricates one",
  ],
  lb016LegacyInsertBefore: "copy-success feedback reserves layout space and causes no layout shift",
});

export function hasExactG3HumanReviewAmendment(prs) {
  const lb015 = prs?.["LB-015"];
  const lb016 = prs?.["LB-016"];
  if (!Array.isArray(lb015?.required_artifacts)
    || !Array.isArray(lb015?.required_tests)
    || !Array.isArray(lb016?.required_artifacts)
    || !Array.isArray(lb016?.required_tests)) return false;
  const [currentArtifact, oldArtifact] = G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016ArtifactReplacement;
  return G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb015AddedArtifacts.every((item) => lb015.required_artifacts.includes(item))
    && G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb015AddedTests.every((item) => lb015.required_tests.includes(item))
    && lb016.required_artifacts.includes(currentArtifact)
    && !lb016.required_artifacts.includes(oldArtifact)
    && G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016AddedTests.every((item) => lb016.required_tests.includes(item))
    && G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016ReplacedTests.every(([current, old]) => lb016.required_tests.includes(current) && !lb016.required_tests.includes(old))
    && G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016RemovedTests.every((item) => !lb016.required_tests.includes(item));
}

export function normalizeG3HumanReviewAmendment(prs) {
  const normalized = structuredClone(prs ?? null);
  if (!hasExactG3HumanReviewAmendment(normalized)) return normalized;
  const lb015 = normalized["LB-015"];
  const lb016 = normalized["LB-016"];
  lb015.required_artifacts = lb015.required_artifacts.filter((item) => !G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb015AddedArtifacts.includes(item));
  lb015.required_tests = lb015.required_tests.filter((item) => !G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb015AddedTests.includes(item));
  const [currentArtifact, oldArtifact] = G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016ArtifactReplacement;
  lb016.required_artifacts = lb016.required_artifacts.map((item) => item === currentArtifact ? oldArtifact : item);
  lb016.required_tests = lb016.required_tests
    .filter((item) => !G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016AddedTests.includes(item))
    .map((item) => {
      const replacement = G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016ReplacedTests.find(([current]) => current === item);
      return replacement ? replacement[1] : item;
    });
  const insertAt = lb016.required_tests.indexOf(G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016LegacyInsertBefore);
  if (insertAt >= 0) lb016.required_tests.splice(insertAt, 0, ...G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.lb016RemovedTests);
  return normalized;
}

function normalizeAuthorizedSemanticCorrections(prs) {
  const normalized = normalizeG3HumanReviewAmendment(prs);
  const lb016 = normalized?.["LB-016"];
  if (Array.isArray(lb016?.required_artifacts)) {
    lb016.required_artifacts = lb016.required_artifacts.filter((item) =>
      item !== "6-screen wizard"
      && !LB016_AUTHORIZED_G3_REWORK_2026_08_13.addedArtifacts.includes(item));
  }
  if (Array.isArray(lb016?.required_tests)) {
    lb016.required_tests = lb016.required_tests
      .filter((item) => !LB016_AUTHORIZED_G3_REWORK_2026_08_13.addedTests.includes(item))
      .map((item) => {
        if (item === LB016_AUTHORIZED_G3_REWORK_2026_08_13.newSuccessTest) return LB016_AUTHORIZED_G3_REWORK_2026_08_13.oldSuccessTest;
        const replacement = LB016_AUTHORIZED_G3_REWORK_2026_08_13.replacedTests.find(([current]) => current === item);
        return replacement ? replacement[1] : item;
      });
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
  if ((contractsDoc?.schema_version ?? 0) >= G3_HUMAN_REVIEW_AMENDMENT_2026_08_13.schemaVersion
    && !hasExactG3HumanReviewAmendment(contractsDoc?.prs)) {
    findings.push(`${expected.id}:human-review-contract-amendment-drift`);
  }
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
