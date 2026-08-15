import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { resolve, relative, join } from "node:path";
import { spawnSync } from "node:child_process";
import { classifyArchitectureRules } from "./core.mjs";
import { validateG4HumanGate, validatePreG4GateAuthorization } from "./g4-human-gate.mjs";
import { validateGovernanceAuthorizations } from "./governance-authorization.mjs";
import { runPrScopedVerifier } from "./pr-scoped.mjs";

const args = process.argv.slice(2);
const valueAfter = (flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
const rootArg = valueAfter("--root");
const root = resolve(rootArg && !rootArg.startsWith("--") ? rootArg : ".");
const progressArg = valueAfter("--progress");
const expectFailure = args.includes("--expect-failure");
const expectedIds = new Set((valueAfter("--expected") ?? "").split(",").map((v) => v.trim()).filter(Boolean));
const repoRoot = resolve(".");
const rulesDoc = JSON.parse(readFileSync(join(repoRoot, "ARCHITECTURE_RULES.json"), "utf8"));
if (rulesDoc.schema_version !== 9 || !Array.isArray(rulesDoc.rules) || rulesDoc.rules.length !== 28) throw new Error("architecture rule inventory must be schema 9 with exactly 28 rules");
const progressPath = resolve(progressArg && !progressArg.startsWith("--") ? progressArg : join(repoRoot, "PR_INDEX.json"));
const progressDoc = JSON.parse(readFileSync(progressPath, "utf8"));
const contractsDoc = JSON.parse(readFileSync(join(repoRoot, "PR_CONTRACTS.json"), "utf8"));

const supportedTypes = new Set([
  "frontend_process_ownership",
  "system_python_fallback",
  "socket_bind_address_policy",
  "whole_app_elevation",
  "self_update_absence",
  "telemetry_absence",
  "visual_dependency_absence",
  "group_review_gate",
  "task_summary_redaction",
  "privileged_broker_install_trust",
  "reviewed_elevated_exec",
]);
const classification = classifyArchitectureRules(rulesDoc, progressDoc, supportedTypes);

const ignoredSegments = new Set(["node_modules", ".git", ".coding-tools", "target", "artifacts"]);
function filesUnder(dir) {
  if (!existsSync(dir)) return [];
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const s = statSync(p);
    if (s.isDirectory()) {
      if (ignoredSegments.has(name)) continue;
      if (resolve(p) === resolve(repoRoot, "scripts", "verify-architecture")) continue;
      out.push(...filesUnder(p));
    } else if (/\.(ts|tsx|js|mjs|rs|json|toml)$/.test(name)) {
      out.push(p);
    }
  }
  return out;
}

const all = root === repoRoot
  ? [...filesUnder(join(root, "src")), ...filesUnder(join(root, "src-tauri")), ...filesUnder(join(root, "scripts"))]
  : filesUnder(root);
const rel = (p) => relative(root, p).replaceAll("\\", "/");
const read = (p) => readFileSync(p, "utf8");
const packageJson = () => JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const dependencyNames = () => Object.keys({ ...(packageJson().dependencies ?? {}), ...(packageJson().devDependencies ?? {}) });
const reviewGovernancePaths = new Set(["PR_INDEX.json", "PROJECT_STATE.json"]);
const runGit = (gitArgs) => spawnSync("git", gitArgs, { cwd: repoRoot, encoding: "utf8", windowsHide: true });
const gitCommitPaths = (commit) => {
  const result = runGit(["-c", "core.quotePath=false", "show", "--format=", "--name-only", commit]);
  if (result.status !== 0) return null;
  return result.stdout.split(/\r?\n/).map((v) => v.trim().replaceAll("\\", "/")).filter(Boolean);
};
const gitJsonAt = (revision, path) => {
  const result = runGit(["show", `${revision}:${path}`]);
  if (result.status !== 0) return null;
  try { return JSON.parse(result.stdout); } catch { return null; }
};
const gitTextAt = (revision, path) => {
  const result = runGit(["show", `${revision}:${path}`]);
  return result.status === 0 ? result.stdout : null;
};
const workingText = (path) => existsSync(join(repoRoot, path)) ? readFileSync(join(repoRoot, path), "utf8") : null;
const gitCommitExists = (commit) => runGit(["cat-file", "-e", `${commit}^{commit}`]).status === 0;
const gitFirstParentChild = (commit) => {
  const result = runGit(["rev-list", "--first-parent", "--reverse", `${commit}..HEAD`]);
  if (result.status !== 0) return null;
  return result.stdout.split(/\r?\n/).map((value) => value.trim()).filter(Boolean)[0] ?? null;
};
const gitFirstParentParent = (commit) => {
  const result = runGit(["show", "-s", "--format=%P", commit]);
  if (result.status !== 0) return null;
  return result.stdout.trim().split(/\s+/)[0] ?? null;
};
const addSourceMatches = (ruleId, predicate, pattern, findings) => {
  for (const p of all) {
    const r = rel(p);
    if (predicate(r) && pattern.test(read(p))) findings.push([ruleId, r]);
  }
};

const verifiers = {
  frontend_process_ownership(rule, findings) {
    addSourceMatches(rule.id, (r) => r.startsWith("src/"), /@tauri-apps\/plugin-shell|(?:node:)?child_process|Command\.create|new\s+Command\s*\(/, findings);
  },
  system_python_fallback(rule, findings) {
    addSourceMatches(rule.id, (r) => r.startsWith("src-tauri/") || (r.startsWith("scripts/") && !r.startsWith("scripts/verify-architecture/")), /python\s+from\s+PATH|py(?:\.exe)?\s+fallback|user\s+site-packages/i, findings);
  },
  socket_bind_address_policy(rule, findings) {
    for (const p of all) {
      const literals = [...read(p).matchAll(/["'`]([^"'`\r\n]*)["'`]/g)].map((m) => m[1].trim());
      if (literals.some((v) => v === "0.0.0.0" || v === "::" || /^0\.0\.0\.0:\d+$/.test(v) || /^\[::\]:\d+$/.test(v))) findings.push([rule.id, rel(p)]);
    }
  },
  whole_app_elevation(rule, findings) {
    addSourceMatches(rule.id, () => true, /requireAdministrator|requestedExecutionLevel[^\n]{0,120}requireAdministrator/i, findings);
  },
  self_update_absence(rule, findings) {
    if (existsSync(join(root, "package.json"))) {
      for (const dep of dependencyNames()) if (/tauri.*updater|updater.*tauri|electron-updater/i.test(dep)) findings.push([rule.id, `package.json:${dep}`]);
    }
    addSourceMatches(rule.id, () => true, /plugin[_-]?updater|checkForUpdates|installUpdate/i, findings);
  },
  telemetry_absence(rule, findings) {
    if (existsSync(join(root, "package.json"))) {
      for (const dep of dependencyNames()) if (/sentry|posthog|mixpanel|amplitude|segment|telemetry|analytics/i.test(dep)) findings.push([rule.id, `package.json:${dep}`]);
    }
    addSourceMatches(rule.id, (r) => r.startsWith("src/") || r.startsWith("src-tauri/"), /Sentry\.init|posthog\.init|mixpanel\.init|amplitude\.init/i, findings);
  },
  visual_dependency_absence(rule, findings) {
    if (!existsSync(join(root, "package.json"))) return;
    for (const dep of dependencyNames()) if (/mui|chakra|antd|bootstrap|tailwind|framer-motion|lucide|heroicons|fontawesome|react-icons|styled-components|emotion/i.test(dep)) findings.push([rule.id, `package.json:${dep}`]);
  },
  group_review_gate(rule, findings) {
    const path = join(root, "PR_INDEX.json");
    if (!existsSync(path)) return;
    const pr = progressDoc;
    const groups = pr.groups ?? [];
    const governanceGit = {
      isAncestor(commit) {
        return runGit(["merge-base", "--is-ancestor", commit, "HEAD"]).status === 0;
      },
      commitPaths: gitCommitPaths,
      commitExists: gitCommitExists,
      jsonAt: gitJsonAt,
      textAt: gitTextAt,
      workingText,
      firstParentChild: gitFirstParentChild,
      firstParentParent: gitFirstParentParent,
    };
    const authorizationFindings = validateGovernanceAuthorizations(contractsDoc, governanceGit);
    for (const detail of authorizationFindings) {
      findings.push([rule.id, `PR_CONTRACTS.json:governance-authorization:${detail}`]);
    }
    for (const detail of validatePreG4GateAuthorization(contractsDoc, governanceGit)) {
      findings.push([rule.id, `PR_CONTRACTS.json:pre-g4-authorization:${detail}`]);
    }
    for (const detail of validateG4HumanGate(pr, governanceGit)) {
      findings.push([rule.id, `PR_INDEX.json:pre-g4-human-gate:${detail}`]);
    }
    for (const group of groups) {
      if (!new Set(["PASS", "FAIL"]).has(group.review_status)) continue;
      const provenance = group.review_provenance;
      if (!provenance
        || provenance.generation !== group.review_generation
        || provenance.kind !== "independent_adversarial"
        || !/^[0-9a-f]{40}$/.test(provenance.commit ?? "")) {
        findings.push([rule.id, `PR_INDEX.json:${group.id}:review-provenance`]);
        continue;
      }

      const ancestor = runGit(["merge-base", "--is-ancestor", provenance.commit, "HEAD"]);
      const paths = gitCommitPaths(provenance.commit);
      if (ancestor.status !== 0
        || !paths
        || paths.length === 0
        || paths.some((candidate) => !reviewGovernancePaths.has(candidate))) {
        findings.push([rule.id, `PR_INDEX.json:${group.id}:review-commit-scope`]);
      }

      const before = gitJsonAt(`${provenance.commit}^`, "PR_INDEX.json");
      const after = gitJsonAt(provenance.commit, "PR_INDEX.json");
      const beforeGroup = before?.groups?.find((candidate) => candidate.id === group.id);
      const afterGroup = after?.groups?.find((candidate) => candidate.id === group.id);
      if (beforeGroup?.status !== "REVIEW_REQUIRED"
        || afterGroup?.review_generation !== provenance.generation
        || afterGroup?.review_status !== group.review_status) {
        findings.push([rule.id, `PR_INDEX.json:${group.id}:review-transition`]);
      }

      for (const invalidated of provenance.invalidated_generations ?? []) {
        if (!Number.isInteger(invalidated.generation)
          || invalidated.generation >= provenance.generation
          || !/^[0-9a-f]{40}$/.test(invalidated.commit ?? "")) {
          findings.push([rule.id, `PR_INDEX.json:${group.id}:invalidated-review-record`]);
          continue;
        }
        if (invalidated.cause === "mixed_scope_review_commit") {
          const invalidatedPaths = gitCommitPaths(invalidated.commit);
          if (!invalidatedPaths || invalidatedPaths.every((candidate) => reviewGovernancePaths.has(candidate))) {
            findings.push([rule.id, `PR_INDEX.json:${group.id}:invalidated-review-evidence`]);
          }
        }
      }
    }
    for (let i = 1; i < groups.length; i += 1) {
      const previous = groups[i - 1];
      const current = groups[i];
      if (current.status !== "BLOCKED" && previous.review_status !== "PASS") findings.push([rule.id, `PR_INDEX.json:${previous.id}->${current.id}`]);
    }
  },
  task_summary_redaction(rule, findings) {
    const taskPath = join(root, "src-tauri", "src", "state", "task.rs");
    if (!existsSync(taskPath)) {
      findings.push([rule.id, "src-tauri/src/state/task.rs:missing"]);
      return;
    }
    const body = read(taskPath);
    const requiredPatterns = [
      [/pub\s+enum\s+SafeTaskSummary\b/, "SafeTaskSummary"],
      [/pub\s+fn\s+from_untrusted\s*\(/, "from_untrusted"],
      [/summary:\s*SafeTaskSummary\b/, "typed-summary-field"],
      [/summary:\s*SafeTaskSummary::from_untrusted\s*\(\s*raw_summary\s*\)/, "start-redaction"],
      [/contains_sensitive_key_value\s*\(/, "sensitive-key-detector"],
    ];
    for (const [pattern, label] of requiredPatterns) {
      if (!pattern.test(body)) findings.push([rule.id, `src-tauri/src/state/task.rs:${label}`]);
    }
    for (const marker of ["token", "password", "access_token", "api_key", "client_secret", "authorization"]) {
      if (!body.toLowerCase().includes(`\"${marker}\"`)) findings.push([rule.id, `src-tauri/src/state/task.rs:missing-${marker}`]);
    }
    for (const p of all) {
      const r = rel(p);
      if (r === "src-tauri/src/state/task.rs" || (!r.startsWith("src-tauri/") && !r.startsWith("src/"))) continue;
      if (/SafeTaskSummary::Text\s*\(/.test(read(p))) findings.push([rule.id, `${r}:direct-SafeTaskSummary-Text`]);
    }
  },
  privileged_broker_install_trust(rule, findings) {
    const configPath = join(root, "src-tauri", "tauri.conf.json");
    const windowsPath = join(root, "src-tauri", "src", "privilege", "windows.rs");
    if (!existsSync(configPath)) {
      findings.push([rule.id, "src-tauri/tauri.conf.json:missing"]);
      return;
    }
    const config = JSON.parse(read(configPath));
    if (config.bundle?.windows?.nsis?.installMode !== "perMachine") {
      findings.push([rule.id, "src-tauri/tauri.conf.json:nsis-installMode"]);
    }
    if (!existsSync(windowsPath)) {
      findings.push([rule.id, "src-tauri/src/privilege/windows.rs:missing"]);
      return;
    }
    const body = read(windowsPath);
    for (const marker of [
      "validate_broker_executable_for_current_install",
      "std::env::current_exe",
      "symlink_metadata",
      "canonicalize",
      "protected_machine_install_root",
      "SHGetFolderPathW",
      "CSIDL_PROGRAM_FILES",
      "trusted_broker",
      "verify_broker_installation_not_mutable_by_unprivileged_principal",
      "validate_install_object_security",
      "GetNamedSecurityInfoW",
      "OWNER_SECURITY_INFORMATION",
      "DACL_SECURITY_INFORMATION",
      "TRUSTED_INSTALL_MUTATION_SIDS",
      "INSTALL_MUTATION_MASK",
      "require_each_access_right_denied",
      "ERROR_ACCESS_DENIED",
      "FILE_WRITE_DATA",
      "FILE_ADD_FILE",
      "FILE_DELETE_CHILD",
      "WRITE_DAC_ACCESS",
      "WRITE_OWNER_ACCESS",
    ]) {
      if (!body.includes(marker)) findings.push([rule.id, `src-tauri/src/privilege/windows.rs:${marker}`]);
    }
    const trustStart = body.indexOf("fn validate_broker_executable_for_current_install");
    const trustEnd = body.indexOf("fn validate_broker_executable(", trustStart);
    const trust = body.slice(trustStart, trustEnd);
    if (!trust.includes("verify_broker_installation_not_mutable_by_unprivileged_principal") || !trust.includes("Ok(trusted_broker)")) {
      findings.push([rule.id, "src-tauri/src/privilege/windows.rs:acl-mutation-check-before-trust"]);
    }
    const objectSecurityStart = body.indexOf("fn validate_install_object_security");
    const objectSecurityEnd = body.indexOf("fn sid_to_string", objectSecurityStart);
    const objectSecurity = body.slice(objectSecurityStart, objectSecurityEnd);
    for (const marker of ["GetNamedSecurityInfoW", "owner.is_null()", "dacl.is_null()", "trusted_install_mutation_sid", "INSTALL_MUTATION_MASK", "ACCESS_ALLOWED_ACE_KIND", "ACCESS_DENIED_ACE_KIND"]) {
      if (!objectSecurity.includes(marker)) findings.push([rule.id, `src-tauri/src/privilege/windows.rs:object-security:${marker}`]);
    }
    const accessStart = body.indexOf("fn require_each_access_right_denied");
    const accessEnd = body.indexOf("fn build_uac_parameters", accessStart);
    const access = body.slice(accessStart, accessEnd);
    if (!access.includes("for desired_access in mutation_rights") || !access.includes("last_error_code() != ERROR_ACCESS_DENIED")) {
      findings.push([rule.id, "src-tauri/src/privilege/windows.rs:per-right-fail-closed-access-probe"]);
    }
  },
  reviewed_elevated_exec(rule, findings) {
    const policyTomlPath = join(root, "runtime-policy.toml");
    const policyPath = join(root, "src-tauri", "src", "mcp", "policy.rs");
    const guardPath = join(root, "src-tauri", "src", "mcp", "guard.rs");
    const serverPath = join(root, "src-tauri", "src", "mcp", "server.rs");
    for (const [path, label] of [[policyTomlPath, "runtime-policy.toml"], [policyPath, "policy.rs"], [guardPath, "guard.rs"], [serverPath, "server.rs"]]) {
      if (!existsSync(path)) findings.push([rule.id, `${label}:missing`]);
    }
    if (![policyTomlPath, policyPath, guardPath, serverPath].every(existsSync)) return;
    const policyToml = read(policyTomlPath);
    for (const marker of [
      'review_model = "exact_trusted_program_and_args"',
      'arbitrary_programs = "deny"',
      'shells_and_interpreters = "deny"',
      'control_plane_mutation = "deny_always"',
      'workdir_policy = "deny_unless_reviewed"',
    ]) {
      if (!policyToml.includes(marker)) findings.push([rule.id, `runtime-policy.toml:${marker}`]);
    }
    const policy = read(policyPath);
    for (const marker of ["decide_request", "reviewed_elevated_exec", "reviewed_elevated_program", "GetSystemDirectoryW", "whoami.exe", "ElevatedExecNotReviewed"]) {
      if (!policy.includes(marker)) findings.push([rule.id, `src-tauri/src/mcp/policy.rs:${marker}`]);
    }
    const guard = read(guardPath);
    if (!guard.includes("decide_request(mode, &request.name, &indirect_capabilities, &request.arguments)")) {
      findings.push([rule.id, "src-tauri/src/mcp/guard.rs:actual-arguments-policy"]);
    }
    const server = read(serverPath);
    const start = server.indexOf("fn handle_elevated_exec");
    const end = server.indexOf("fn request_id", start);
    const handler = start >= 0 && end > start ? server.slice(start, end) : "";
    if (!handler.includes("arguments.clone()") || handler.includes('ToolCallRequest::new("elevated_exec", json!({}))')) {
      findings.push([rule.id, "src-tauri/src/mcp/server.rs:actual-elevated-arguments"]);
    }
    if (!handler.includes("let execution_guard = guard") || !handler.includes("drop(execution_guard)")) {
      findings.push([rule.id, "src-tauri/src/mcp/server.rs:single-execution-gate"]);
    }
  },
};

const findings = [];
let builtInActive = 0;
let prScopedActive = 0;
for (const rule of classification.activeRules) {
  if (supportedTypes.has(rule.verification.type)) {
    verifiers[rule.verification.type](rule, findings);
    builtInActive += 1;
    continue;
  }
  if (rule.verification.mode !== "deferred") {
    throw new Error(`${rule.id} enforced rule cannot use a PR-scoped verifier`);
  }
  try {
    runPrScopedVerifier(repoRoot, rule, contractsDoc);
    prScopedActive += 1;
  } catch (error) {
    findings.push([rule.id, `pr-scoped:${error.message}`]);
  }
}

const actualIds = new Set(findings.map(([id]) => id));
if (expectFailure) {
  if (expectedIds.size === 0) throw new Error("--expect-failure requires --expected ARCH-xxx,...");
  const missing = [...expectedIds].filter((id) => !actualIds.has(id));
  const unexpected = [...actualIds].filter((id) => !expectedIds.has(id));
  if (missing.length || unexpected.length) throw new Error(`architecture negative fixture mismatch missing=${missing.join("|") || "none"} unexpected=${unexpected.join("|") || "none"}`);
  console.log(`ARCHITECTURE_NEGATIVE_FIXTURE=PASS expected=${[...expectedIds].sort().join(",")}`);
  process.exit(0);
}
if (findings.length) {
  for (const [id, file] of findings) console.error(`${id}: ${file}`);
  process.exit(1);
}
console.log(`ARCHITECTURE_VERIFY=PASS configured_enforced=${classification.configuredEnforced.length} activated_deferred=${classification.activatedDeferred.length} future_deferred=${classification.futureDeferred.length} built_in_active=${builtInActive} pr_scoped_active=${prScopedActive} active=${classification.activeRules.length} total=${rulesDoc.rules.length}`);
