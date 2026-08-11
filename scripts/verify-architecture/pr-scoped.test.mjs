import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { classifyArchitectureRules } from "./core.mjs";
import { resolvePrScopedVerifier, runPrScopedVerifier, writablePatternMatches } from "./pr-scoped.mjs";

const supported = new Set(["built_in"]);
const rule = {
  id: "ARCH-017",
  verification: { mode: "deferred", activate_at_pr: "LB-003", reason: "fixture" },
};
const progress = (status) => ({
  execution: { execution_order: ["LB-001", "LB-002", "LB-003"] },
  prs: [
    { id: "LB-001", status: "PASS" },
    { id: "LB-002", status: "PASS" },
    { id: "LB-003", status },
  ],
});
const contracts = { prs: { "LB-003": { writable_paths: ["tests/unit/workspace/**", "src-tauri/src/workspace/**"] } } };

assert.equal(writablePatternMatches("tests/unit/workspace/ARCH-017.verify.mjs", "tests/unit/workspace/**"), true);
assert.equal(writablePatternMatches("tests/unit/settings/a.rs", "tests/unit/workspace/**"), false);
assert.equal(writablePatternMatches("vite.config.ts", "vite.config.*"), true);

const future = classifyArchitectureRules({ rules: [rule] }, progress("BLOCKED"), supported);
assert.equal(future.futureDeferred.length, 1);

const active = classifyArchitectureRules({ rules: [rule] }, progress("IN_PROGRESS"), supported);
assert.equal(active.activatedDeferred.length, 1);

const makeRoot = () => mkdtempSync(join(tmpdir(), "localbridge-arch-"));
const scopedPath = (root, body = "process.exit(0);\n") => {
  const dir = join(root, "tests", "unit", "workspace");
  mkdirSync(dir, { recursive: true });
  const path = join(dir, "ARCH-017.verify.mjs");
  writeFileSync(path, body);
  return path;
};

let root = makeRoot();
try {
  assert.throws(() => resolvePrScopedVerifier(root, rule, contracts), /without built-in or PR-scoped verifier/);
} finally { rmSync(root, { recursive: true, force: true }); }

root = makeRoot();
try {
  scopedPath(root, "if (!process.env.LOCALBRIDGE_ARCH_RULE_ID || !process.env.LOCALBRIDGE_REPO_ROOT) process.exit(2);\n");
  assert.equal(runPrScopedVerifier(root, rule, contracts), "tests/unit/workspace/ARCH-017.verify.mjs");
} finally { rmSync(root, { recursive: true, force: true }); }

root = makeRoot();
try {
  scopedPath(root, "process.exit(7);\n");
  assert.throws(() => runPrScopedVerifier(root, rule, contracts), /failed .* exit 7/);
} finally { rmSync(root, { recursive: true, force: true }); }

root = makeRoot();
try {
  scopedPath(root);
  const second = join(root, "src-tauri", "src", "workspace");
  mkdirSync(second, { recursive: true });
  writeFileSync(join(second, "ARCH-017.verify.mjs"), "process.exit(0);\n");
  assert.throws(() => resolvePrScopedVerifier(root, rule, contracts), /duplicate PR-scoped verifiers/);
} finally { rmSync(root, { recursive: true, force: true }); }

root = makeRoot();
try {
  const dir = join(root, "scripts");
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "ARCH-017.verify.mjs"), "process.exit(0);\n");
  assert.throws(() => resolvePrScopedVerifier(root, rule, contracts), /outside LB-003 writable paths/);
} finally { rmSync(root, { recursive: true, force: true }); }

console.log("ARCHITECTURE_PR_SCOPED_TEST=PASS future_missing=true active_missing_fail_closed=true passing=true failing_rejected=true duplicate_rejected=true out_of_scope_rejected=true");
