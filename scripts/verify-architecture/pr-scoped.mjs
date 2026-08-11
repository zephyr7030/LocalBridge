import { existsSync, readdirSync, statSync } from "node:fs";
import { relative, resolve, join } from "node:path";
import { spawnSync } from "node:child_process";

const IGNORED_SEGMENTS = new Set(["node_modules", ".git", ".coding-tools", "target", "artifacts"]);

function normalize(path) {
  return path.replaceAll("\\", "/").replace(/^\.\//, "");
}

function escapeRegex(value) {
  return value.replace(/[.+^${}()|[\]\\]/g, "\\$&");
}

export function writablePatternMatches(path, pattern) {
  const normalizedPath = normalize(path);
  const normalizedPattern = normalize(pattern);
  let source = "";
  for (let i = 0; i < normalizedPattern.length; i += 1) {
    const ch = normalizedPattern[i];
    if (ch === "*" && normalizedPattern[i + 1] === "*") {
      source += ".*";
      i += 1;
    } else if (ch === "*") {
      source += "[^/]*";
    } else {
      source += escapeRegex(ch);
    }
  }
  return new RegExp(`^${source}$`).test(normalizedPath);
}

function verifierFilesUnder(dir, fileName, repoRoot, out) {
  if (!existsSync(dir)) return;
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      if (IGNORED_SEGMENTS.has(name)) continue;
      verifierFilesUnder(path, fileName, repoRoot, out);
    } else if (name === fileName) {
      out.push(normalize(relative(repoRoot, path)));
    }
  }
}

export function resolvePrScopedVerifier(repoRoot, rule, contractsDoc) {
  const activationPr = rule?.verification?.activate_at_pr;
  if (!activationPr) throw new Error(`${rule?.id ?? "unknown"} missing activate_at_pr`);
  const writable = contractsDoc?.prs?.[activationPr]?.writable_paths;
  if (!Array.isArray(writable) || writable.length === 0) {
    throw new Error(`${rule.id} activation PR ${activationPr} has no writable_paths contract`);
  }
  const fileName = `${rule.id}.verify.mjs`;
  const allCandidates = [];
  verifierFilesUnder(resolve(repoRoot), fileName, resolve(repoRoot), allCandidates);
  const inScope = allCandidates.filter((path) => writable.some((pattern) => writablePatternMatches(path, pattern)));
  const outOfScope = allCandidates.filter((path) => !inScope.includes(path));
  if (outOfScope.length) {
    throw new Error(`${rule.id} PR-scoped verifier exists outside ${activationPr} writable paths: ${outOfScope.sort().join("|")}`);
  }
  if (inScope.length === 0) {
    throw new Error(`${rule.id} activated at ${activationPr} without built-in or PR-scoped verifier`);
  }
  if (inScope.length !== 1) {
    throw new Error(`${rule.id} has duplicate PR-scoped verifiers: ${inScope.sort().join("|")}`);
  }
  return inScope[0];
}

export function runPrScopedVerifier(repoRoot, rule, contractsDoc) {
  const modulePath = resolvePrScopedVerifier(repoRoot, rule, contractsDoc);
  const result = spawnSync(process.execPath, [resolve(repoRoot, modulePath)], {
    cwd: resolve(repoRoot),
    env: {
      ...process.env,
      LOCALBRIDGE_ARCH_RULE_ID: rule.id,
      LOCALBRIDGE_ARCH_ACTIVATE_AT_PR: rule.verification.activate_at_pr,
      LOCALBRIDGE_REPO_ROOT: resolve(repoRoot),
    },
    encoding: "utf8",
    shell: false,
    windowsHide: true,
  });
  if (result.error) {
    throw new Error(`${rule.id} PR-scoped verifier could not start: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(`${rule.id} PR-scoped verifier failed at ${modulePath} with exit ${result.status}`);
  }
  return modulePath;
}
