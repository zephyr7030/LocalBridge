import assert from "node:assert/strict";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { auditPackageRoot, scanTextForSensitive } from "../../../scripts/public-release/preflight.mjs";
import { isPrivatePath, isPublicPath, sanitizePublicText } from "../../../scripts/public-release/policy.mjs";

assert.equal(isPrivatePath("PR_CONTRACTS.json"), true);
assert.equal(isPrivatePath("governance/G4_PRE_RELEASE_AUTHORIZATION.txt"), true);
assert.equal(isPrivatePath(".coding-tools/task-state.json"), true);
assert.equal(isPrivatePath("runtime/tunnel-client/cloudflared.exe"), true);
assert.equal(isPublicPath("src/App.tsx"), true);
assert.equal(isPublicPath("src-tauri/src/lib.rs"), true);
assert.equal(isPublicPath("README.md"), true);
assert.equal(isPublicPath("tests/integration/mcp/coding_runtime.rs"), true);
assert.equal(isPublicPath("tests/e2e/dashboard/lb015_contract.test.mjs"), false);
assert.equal(isPublicPath("tests/integration/release-preflight/lb018pre.test.mjs"), true);

const manifest = '[verification]\ncompatibility_gate = "LB-000"\npackaging_gate = "LB-018"\n\n[privileged_broker]\nkind = "first_party_rust_binary"\n';
const sanitizedManifest = sanitizePublicText("runtime-manifest.toml", manifest);
assert.equal(sanitizedManifest.includes("LB-018"), false);
assert.equal(sanitizedManifest.includes("[privileged_broker]"), true);
assert.equal(sanitizePublicText("runtime-policy.toml", 'status = "LB_007_STABLE_PUBLIC_POLICY"\n').trim(), 'status = "stable_public_policy"');

const syntheticSecret = "sk-" + "A".repeat(36);
assert.equal(scanTextForSensitive(`Runtime API Key=${syntheticSecret}`, "fixture").high.length, 1);
assert.equal(scanTextForSensitive(`synthetic test token=${syntheticSecret}`, "fixture").high.length, 0);
assert.equal(scanTextForSensitive("path=C:\\Users\\alice\\Desktop\\dump.txt", "fixture").warnings.length > 0, true);

const fixture = resolve("release-artifacts/preflight/lb018pre-package-fixture");
await rm(fixture, { recursive: true, force: true });
await mkdir(fixture, { recursive: true });
await writeFile(resolve(fixture, "LocalBridge.exe"), "synthetic binary placeholder");
assert.equal((await auditPackageRoot(fixture)).ok, true);
await writeFile(resolve(fixture, "PR_CONTRACTS.json"), "{}");
assert.equal((await auditPackageRoot(fixture)).ok, false);
await rm(fixture, { recursive: true, force: true });

console.log("PUBLIC_RELEASE_PREFLIGHT_TEST=PASS private_governance=false fresh_history_policy=true package_audit=true");
