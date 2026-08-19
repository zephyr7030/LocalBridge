import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { dirname, extname, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { isPrivatePath, isPublicPath, normalizeRepoPath, PUBLIC_FORBIDDEN_TEXT, sanitizePublicText } from "./policy.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const evidenceRoot = resolve(root, "release-artifacts", "preflight");
const MAX_TEXT_BYTES = 16 * 1024 * 1024;
const PRIVATE_MARKERS = /(?:PR_CONTRACTS\.json|PROJECT_STATE\.json|PR_INDEX\.json|FINAL_REVIEW\.json|governance\/G4_)/i;
const PLACEHOLDER = /(?:test|fake|dummy|example|synthetic|redacted|placeholder|your[_ -]|<[^>]+>|env:|\$\{|sha256)/i;
const MACHINE_PATH = /(?:[A-Za-z]:\\Users\\[^\\\s"']+|\/Users\/[^\/\s"']+|\/home\/[^\/\s"']+|S-1-5-21-(?:\d+-){2,}\d+)/gi;
const SECRET_PATTERNS = [
  ["private_key", /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/g],
  ["openai_key", /\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{24,}\b/g],
  ["github_token", /\bgh[pousr]_[A-Za-z0-9]{30,}\b/g],
  ["aws_access_key", /\bAKIA[0-9A-Z]{16}\b/g],
  ["bearer_token", /Authorization\s*[:=]\s*Bearer\s+[A-Za-z0-9._~+\/-]{24,}/gi],
  ["named_secret", /(?:RUNTIME_API_KEY|CONTROL_PLANE_TUNNEL_ID|AUTH_TOKEN|API_KEY|ACCESS_TOKEN|SECRET)\s*[:=]\s*["'][^"'\r\n]{16,}["']/gi],
];

const sha256 = (value) => createHash("sha256").update(value).digest("hex");
const run = (program, args, options = {}) => {
  const windowsNpm = process.platform === "win32" && program === "npm";
  const executable = windowsNpm ? (process.env.ComSpec || "cmd.exe") : program;
  const spawnArgs = windowsNpm ? ["/d", "/s", "/c", "npm.cmd", ...args] : args;
  const result = spawnSync(executable, spawnArgs, {
    cwd: options.cwd ?? root,
    env: options.env ?? process.env,
    encoding: "utf8",
    stdio: options.stdio ?? "pipe",
    windowsHide: true,
    maxBuffer: options.maxBuffer ?? 256 * 1024 * 1024,
  });
  if (result.status !== 0) {
    if (result.error) throw new Error(`${program} ${args.join(" ")} could not start: ${result.error.message}`);
    const stderr = String(result.stderr ?? "").trim();
    const stdout = String(result.stdout ?? "").trim();
    throw new Error(`${program} ${args.join(" ")} failed (${result.status ?? "unknown"})${stderr ? `: ${stderr}` : stdout ? `: ${stdout}` : ""}`);
  }
  return String(result.stdout ?? "");
};
const git = (args, options = {}) => run("git", args, options);
const writeReport = async (name, data) => {
  await mkdir(evidenceRoot, { recursive: true });
  const path = resolve(evidenceRoot, name);
  await writeFile(path, `${JSON.stringify(data, null, 2)}\n`, "utf8");
  return path;
};
const isLikelyText = (buffer) => !buffer.subarray(0, Math.min(buffer.length, 8192)).includes(0);
const safeSample = (item) => ({ source: item.source, path: item.path ?? null, commit: item.commit ?? null, line: item.line ?? null, category: item.category });

export function scanTextForSensitive(text, source = "text", meta = {}) {
  const high = [];
  const warnings = [];
  const lines = String(text).split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (!PLACEHOLDER.test(line)) {
      for (const [category, regex] of SECRET_PATTERNS) {
        regex.lastIndex = 0;
        if (regex.test(line)) high.push({ source, category, line: index + 1, ...meta });
      }
    }
    MACHINE_PATH.lastIndex = 0;
    if (MACHINE_PATH.test(line)) warnings.push({ source, category: "machine_identity_path", line: index + 1, ...meta });
  }
  return { high, warnings };
}

async function scanTrackedFiles() {
  const paths = git(["ls-files", "-z"]).split("\0").filter(Boolean);
  const high = [];
  const warnings = [];
  const skipped = [];
  for (const repoPath of paths) {
    const absolute = resolve(root, repoPath);
    if (!existsSync(absolute)) continue;
    const info = await stat(absolute);
    if (!info.isFile()) continue;
    if (info.size > MAX_TEXT_BYTES) {
      skipped.push({ path: repoPath, reason: "large_or_binary_candidate", bytes: info.size });
      continue;
    }
    const buffer = await readFile(absolute);
    if (!isLikelyText(buffer)) continue;
    const result = scanTextForSensitive(buffer.toString("utf8"), "tracked", { path: normalizeRepoPath(repoPath) });
    high.push(...result.high);
    warnings.push(...result.warnings);
    if (/\.(?:dmp|dump|core)$/i.test(repoPath)) warnings.push({ source: "tracked", path: repoPath, category: "debug_dump", line: null });
  }
  return { high, warnings, skipped, tracked_count: paths.length };
}

function scanReachableHistory() {
  const patch = git(["log", "--all", "--format=__LB_COMMIT__%H", "-p", "--no-ext-diff", "--unified=0"], { maxBuffer: 512 * 1024 * 1024 });
  const high = [];
  const warnings = [];
  let commit = null;
  let patchLine = 0;
  for (const line of patch.split(/\r?\n/)) {
    if (line.startsWith("__LB_COMMIT__")) {
      commit = line.slice("__LB_COMMIT__".length);
      patchLine = 0;
      continue;
    }
    patchLine += 1;
    if (!line.startsWith("+") || line.startsWith("+++")) continue;
    const result = scanTextForSensitive(line.slice(1), "history", { commit, line: patchLine });
    high.push(...result.high);
    warnings.push(...result.warnings);
  }
  return { high, warnings, commit_count: Number(git(["rev-list", "--all", "--count"]).trim()) };
}

export async function scanRepositorySensitive() {
  const tracked = await scanTrackedFiles();
  const history = scanReachableHistory();
  const high = [...tracked.high, ...history.high];
  const warnings = [...tracked.warnings, ...history.warnings];
  const report = {
    schema: 1,
    high_confidence_secret_count: high.length,
    machine_or_debug_warning_count: warnings.length,
    tracked_file_count: tracked.tracked_count,
    reachable_commit_count: history.commit_count,
    skipped_large_files: tracked.skipped,
    high_confidence_samples: high.slice(0, 50).map(safeSample),
    warning_samples: warnings.slice(0, 100).map(safeSample),
    note: "Reports never include matched secret values. Machine-path/history warnings are informational because the public repository is generated with fresh history; high-confidence credentials are blocking.",
  };
  const path = await writeReport("sensitive-scan.json", report);
  if (high.length) throw new Error(`sensitive scan found ${high.length} high-confidence credential candidate(s); see ${path}`);
  console.log(`PRE_RELEASE_SENSITIVE_SCAN=PASS commits=${history.commit_count} tracked=${tracked.tracked_count} warnings=${warnings.length}`);
  return report;
}

async function walkFiles(base, current = base, skippedDirectories = new Set()) {
  const files = [];
  for (const entry of await readdir(current, { withFileTypes: true })) {
    const absolute = resolve(current, entry.name);
    const rel = normalizeRepoPath(relative(base, absolute));
    if (entry.isDirectory()) {
      if (skippedDirectories.has(rel)) continue;
      files.push(...await walkFiles(base, absolute, skippedDirectories));
    } else if (entry.isFile()) files.push(absolute);
  }
  return files;
}

export async function auditPackageRoot(packageRoot) {
  const base = resolve(packageRoot);
  const violations = [];
  if (!existsSync(base)) return { ok: false, violations: [{ path: normalizeRepoPath(packageRoot), reason: "missing_package_root" }] };
  for (const absolute of await walkFiles(base)) {
    const rel = normalizeRepoPath(relative(base, absolute));
    const lower = rel.toLowerCase();
    if (isPrivatePath(rel)
      || /(?:^|\/)(?:node_modules|target|tests?|logs?|diagnostics|cache|tmp|temp)(?:\/|$)/i.test(rel)
      || /(?:^|\/)(?:\.env(?:\.|$)|[^/]+\.(?:log|dmp|dump|core|key|pem|pfx|p12))$/i.test(rel)
      || /cloudflared/i.test(rel)
      || /(?:dev|development)[-_ ]?tunnel/i.test(rel)) {
      violations.push({ path: rel, reason: "forbidden_release_path" });
      continue;
    }
    const info = await stat(absolute);
    if (info.size > MAX_TEXT_BYTES) continue;
    const buffer = await readFile(absolute);
    if (!isLikelyText(buffer)) continue;
    const text = buffer.toString("utf8");
    const sensitive = scanTextForSensitive(text, "package", { path: rel });
    if (sensitive.high.length) violations.push({ path: rel, reason: "secret_candidate" });
    if (MACHINE_PATH.test(text)) violations.push({ path: rel, reason: "machine_specific_path" });
    MACHINE_PATH.lastIndex = 0;
    if (PRIVATE_MARKERS.test(text)) violations.push({ path: rel, reason: "private_governance_reference" });
  }
  return { ok: violations.length === 0, violations };
}

async function verifyLicenseInventory() {
  for (const path of ["LICENSE", "THIRD_PARTY_NOTICES.md", "runtime/coding-tools-mcp/LICENSE", "runtime/python/LICENSE.txt", "runtime/tunnel-client/LICENSE"]) {
    if (!existsSync(resolve(root, path))) throw new Error(`required license file missing: ${path}`);
  }
  const lock = JSON.parse(await readFile(resolve(root, "package-lock.json"), "utf8"));
  const missingNpm = Object.entries(lock.packages ?? {}).filter(([key, value]) => key && !value.link && !value.license).map(([key]) => key);
  if (missingNpm.length) throw new Error(`npm lock packages missing license metadata: ${missingNpm.slice(0, 10).join(", ")}`);
  const metadata = JSON.parse(run("cargo", ["metadata", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--format-version", "1"]));
  const missingCargo = metadata.packages.filter((pkg) => pkg.name !== "localbridge" && !pkg.license && !pkg.license_file).map((pkg) => `${pkg.name}@${pkg.version}`);
  if (missingCargo.length) throw new Error(`Cargo packages missing license metadata: ${missingCargo.slice(0, 10).join(", ")}`);
  const notices = await readFile(resolve(root, "THIRD_PARTY_NOTICES.md"), "utf8");
  for (const marker of ["coding-tools-mcp", "tunnel-client", "Python", "Tauri", "WebView2", "aria2", "7-Zip", "jq", "curl.exe"]) {
    if (!notices.includes(marker)) throw new Error(`THIRD_PARTY_NOTICES missing ${marker}`);
  }
  const report = { schema: 1, npm_packages: Object.keys(lock.packages ?? {}).length - 1, cargo_packages: metadata.packages.length - 1, missing_npm_license_metadata: 0, missing_cargo_license_metadata: 0 };
  await writeReport("license-audit.json", report);
  console.log(`PRE_RELEASE_LICENSE_AUDIT=PASS npm=${report.npm_packages} cargo=${report.cargo_packages}`);
  return report;
}

async function cleanCheckoutBuild() {
  const checkout = resolve(evidenceRoot, "clean-checkout");
  const stubBin = resolve(evidenceRoot, "no-python-bin");
  await rm(checkout, { recursive: true, force: true });
  await rm(stubBin, { recursive: true, force: true });
  await mkdir(stubBin, { recursive: true });
  for (const name of ["python.cmd", "python3.cmd", "py.cmd"]) await writeFile(resolve(stubBin, name), "@echo off\r\necho PRE_RELEASE_EXTERNAL_PYTHON_FORBIDDEN 1>&2\r\nexit /b 91\r\n", "utf8");
  git(["clone", "--no-hardlinks", "--local", ".", checkout]);
  const env = { ...process.env, CI: "1", VIRTUAL_ENV: "", PYTHONHOME: "", PYTHONPATH: "", PATH: `${stubBin};${process.env.PATH ?? ""}` };
  run("npm", ["ci"], { cwd: checkout, env, stdio: "inherit" });
  run("npm", ["run", "build"], { cwd: checkout, env, stdio: "inherit" });
  run("node", ["scripts/prepare-lb018-resources.mjs"], { cwd: checkout, env, stdio: "inherit" });
  run("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--release"], { cwd: checkout, env, stdio: "inherit" });
  const report = { schema: 1, head: git(["rev-parse", "HEAD"], { cwd: checkout }).trim(), npm_ci: "PASS", frontend_build: "PASS", release_resource_prepare: "PASS", cargo_release_build: "PASS", external_python_environment: "blocked_by_preflight_stub" };
  await writeReport("clean-checkout-build.json", report);
  console.log(`PRE_RELEASE_CLEAN_CHECKOUT=PASS head=${report.head}`);
  return report;
}

async function verifyPublicTree(base) {
  const violations = [];
  const generated = new Set([".git", "node_modules", "src-tauri/target", "tests/artifacts", "src-tauri/gen"]);
  for (const absolute of await walkFiles(base, base, generated)) {
    const rel = normalizeRepoPath(relative(base, absolute));
    if (isPrivatePath(rel) || !isPublicPath(rel)) violations.push({ path: rel, reason: "not_public_allowlisted" });
    const info = await stat(absolute);
    if (info.size > MAX_TEXT_BYTES) continue;
    const buffer = await readFile(absolute);
    if (!isLikelyText(buffer)) continue;
    const text = buffer.toString("utf8");
    const policyDefinitionFile = rel === "scripts/public-release/policy.mjs" || rel === "scripts/public-release/preflight.mjs" || rel === "tests/integration/release-preflight/public_release.test.mjs";
    if (!policyDefinitionFile) for (const marker of PUBLIC_FORBIDDEN_TEXT) if (text.includes(marker)) violations.push({ path: rel, reason: `private_marker:${marker}` });
    const sensitive = scanTextForSensitive(text, "public_export", { path: rel });
    if (sensitive.high.length) violations.push({ path: rel, reason: "secret_candidate" });
  }
  return violations;
}

async function exportPublicSource() {
  const output = resolve(evidenceRoot, "public-source");
  await rm(output, { recursive: true, force: true });
  await mkdir(output, { recursive: true });
  const tracked = git(["ls-files", "-z"]).split("\0").filter(Boolean);
  let copied = 0;
  let transformed = 0;
  for (const repoPath of tracked) {
    const normalized = normalizeRepoPath(repoPath);
    if (!isPublicPath(normalized)) continue;
    const source = resolve(root, normalized);
    if (!existsSync(source) || !(await stat(source)).isFile()) continue;
    const target = resolve(output, normalized);
    await mkdir(dirname(target), { recursive: true });
    const buffer = await readFile(source);
    if (isLikelyText(buffer)) {
      const original = buffer.toString("utf8");
      const sanitized = sanitizePublicText(normalized, original);
      if (sanitized !== original) transformed += 1;
      await writeFile(target, sanitized, "utf8");
    } else await copyFile(source, target);
    copied += 1;
  }
  const violations = await verifyPublicTree(output);
  if (violations.length) throw new Error(`public source verification failed: ${JSON.stringify(violations.slice(0, 20))}`);
  git(["init", "-b", "main"], { cwd: output });
  git(["config", "user.name", "LocalBridge Release"], { cwd: output });
  git(["config", "user.email", "release@local.invalid"], { cwd: output });
  git(["add", "."], { cwd: output });
  git(["commit", "-m", "Initial public source snapshot"], { cwd: output });
  const commits = Number(git(["rev-list", "--count", "HEAD"], { cwd: output }).trim());
  const remotes = git(["remote"], { cwd: output }).trim();
  if (commits !== 1 || remotes) throw new Error(`fresh-history invariant failed commits=${commits} remotes=${JSON.stringify(remotes)}`);
  const report = { schema: 1, private_source_head: git(["rev-parse", "HEAD"]).trim(), public_source_head: git(["rev-parse", "HEAD"], { cwd: output }).trim(), copied_files: copied, transformed_public_metadata_files: transformed, public_commit_count: commits, remotes: [] };
  await writeReport("public-export.json", report);
  console.log(`PRE_RELEASE_PUBLIC_EXPORT=PASS files=${copied} commits=1 transformed=${transformed}`);
  return report;
}

async function formatCheck() {
  run("git", ["diff", "--check"], { stdio: "inherit" });
  let base = "HEAD^";
  if (process.env.GITHUB_BASE_REF) base = `origin/${process.env.GITHUB_BASE_REF}`;
  let changed = [];
  try { changed = git(["diff", "--name-only", `${base}...HEAD`, "--", "*.rs"]).split(/\r?\n/).filter(Boolean); } catch { changed = []; }
  for (const path of changed) run("rustfmt", ["--edition", "2024", "--check", path], { stdio: "inherit" });
  console.log(`PRE_RELEASE_FORMAT_CHECK=PASS rust_files=${changed.length}`);
}

async function verifyTrackedLocalState() {
  const tracked = git(["ls-files", ".coding-tools"]).trim();
  if (tracked) throw new Error(`generated .coding-tools state remains tracked: ${tracked}`);
  console.log("PRE_RELEASE_LOCAL_STATE=PASS tracked_coding_tools=0");
}

async function main() {
  const command = process.argv[2] ?? "help";
  if (command === "scan-sensitive") return scanRepositorySensitive();
  if (command === "verify-license") return verifyLicenseInventory();
  if (command === "clean-build") return cleanCheckoutBuild();
  if (command === "export-public") return exportPublicSource();
  if (command === "verify-public") {
    const base = resolve(process.argv[3] ?? ".");
    const violations = await verifyPublicTree(base);
    if (violations.length) throw new Error(JSON.stringify(violations.slice(0, 20)));
    console.log("PRE_RELEASE_PUBLIC_TREE=PASS");
    return;
  }
  if (command === "audit-package") {
    const result = await auditPackageRoot(process.argv[3] ?? ".");
    if (!result.ok) throw new Error(JSON.stringify(result.violations.slice(0, 20)));
    console.log("PRE_RELEASE_PACKAGE_AUDIT=PASS");
    return result;
  }
  if (command === "format-check") return formatCheck();
  if (command === "verify-local-state") return verifyTrackedLocalState();
  throw new Error("usage: node scripts/public-release/preflight.mjs <scan-sensitive|verify-license|clean-build|export-public|verify-public|audit-package|format-check|verify-local-state> [path]");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(`PRE_RELEASE_PREFLIGHT=FAIL ${error.message}`);
    process.exitCode = 1;
  });
}
