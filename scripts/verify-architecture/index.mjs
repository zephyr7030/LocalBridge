import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { resolve, relative, join } from "node:path";

const args = process.argv.slice(2);
const valueAfter = (flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
const rootArg = valueAfter("--root");
const root = resolve(rootArg && !rootArg.startsWith("--") ? rootArg : ".");
const expectFailure = args.includes("--expect-failure");
const expectedIds = new Set((valueAfter("--expected") ?? "").split(",").map((v) => v.trim()).filter(Boolean));
const repoRoot = resolve(".");
const rulesDoc = JSON.parse(readFileSync(join(repoRoot, "ARCHITECTURE_RULES.json"), "utf8"));
if (!Array.isArray(rulesDoc.rules) || rulesDoc.rules.length !== 24) throw new Error("architecture rule inventory must contain exactly 24 rules");

const supportedTypes = new Set([
  "frontend_process_ownership",
  "system_python_fallback",
  "socket_bind_address_policy",
  "whole_app_elevation",
  "self_update_absence",
  "telemetry_absence",
  "visual_dependency_absence",
  "group_review_gate",
]);
const ids = new Set();
for (const rule of rulesDoc.rules) {
  if (!/^ARCH-\d{3}$/.test(rule.id) || ids.has(rule.id)) throw new Error(`invalid or duplicate architecture rule id: ${rule.id}`);
  ids.add(rule.id);
  const verification = rule.verification;
  if (!verification || !["enforced", "deferred"].includes(verification.mode)) throw new Error(`${rule.id} missing supported verification mode`);
  if (verification.mode === "enforced" && !supportedTypes.has(verification.type)) throw new Error(`${rule.id} has unsupported verifier type: ${verification.type ?? "missing"}`);
  if (verification.mode === "deferred" && (!/^LB-\d{3}$/.test(verification.activate_at_pr ?? "") || !verification.reason)) throw new Error(`${rule.id} deferred verification requires activate_at_pr and reason`);
}

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
    const pr = JSON.parse(readFileSync(path, "utf8"));
    const groups = pr.groups ?? [];
    for (let i = 1; i < groups.length; i += 1) {
      const previous = groups[i - 1];
      const current = groups[i];
      if (current.status !== "BLOCKED" && previous.review_status !== "PASS") findings.push([rule.id, `PR_INDEX.json:${previous.id}->${current.id}`]);
    }
  },
};

const findings = [];
const enforced = rulesDoc.rules.filter((r) => r.verification.mode === "enforced");
const deferred = rulesDoc.rules.filter((r) => r.verification.mode === "deferred");
for (const rule of enforced) verifiers[rule.verification.type](rule, findings);

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
console.log(`ARCHITECTURE_VERIFY=PASS enforced=${enforced.length} deferred=${deferred.length} total=${rulesDoc.rules.length}`);
