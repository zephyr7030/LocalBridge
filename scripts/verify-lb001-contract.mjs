import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
const sha = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
for (const p of ["package-lock.json", "src-tauri/Cargo.lock", "src-tauri/tauri.conf.json"]) if (!existsSync(p)) throw new Error(`missing ${p}`);
const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
if (!config.bundle.externalBin.includes("binaries/dummy-sidecar")) throw new Error("Tauri externalBin missing dummy sidecar");
if (!config.bundle.targets.includes("nsis")) throw new Error("Tauri NSIS bundle target missing");
if (!config.bundle.icon.includes("../assets/icons/localbridge.ico")) throw new Error("frozen ICO not wired");
const manifest = readFileSync("runtime-manifest.toml", "utf8");
if (!manifest.includes('bundle_webview2 = false') || !manifest.includes('webview2_source = "windows-system-runtime"')) throw new Error("WebView2 contract mismatch");
const buildRs = readFileSync("src-tauri/build.rs", "utf8");
if (!buildRs.includes('x86_64-pc-windows-msvc') || !buildRs.includes('dummy-sidecar-x86_64-pc-windows-msvc.exe')) throw new Error("standalone Cargo dummy sidecar preparation missing");
const cargoToml = readFileSync("src-tauri/Cargo.toml", "utf8");
if (!/^autobins\s*=\s*false$/m.test(cargoToml)) throw new Error("Cargo autobin discovery must be disabled so Tauri externalBin is the only packaged dummy sidecar source");
if (/name\s*=\s*"dummy-sidecar"/.test(cargoToml)) throw new Error("dummy sidecar must not be a Cargo application binary");
if (sha("assets/icons/localbridge.ico") !== "c995d6af01ebc5031950eb9ea6415b58671b31f84ed6b55baabe80ea51e33f78") throw new Error("frozen ICO hash mismatch");
if (sha("assets/icons/localbridge.png") !== "710690f2d70e3c69f13db9d4eaebc0bef5c80561c74acc7bc5a401c15c16e55a") throw new Error("frozen PNG hash mismatch");
const pkg = JSON.parse(readFileSync("package.json", "utf8"));
const deps = Object.keys({ ...(pkg.dependencies ?? {}), ...(pkg.devDependencies ?? {}) });
if (deps.some((d) => /lucide|heroicons|fontawesome|react-icons/i.test(d))) throw new Error("icon library dependency forbidden");
if (!pkg.devDependencies?.["@tauri-apps/cli"]) throw new Error("local Tauri CLI dependency missing");

const rules = JSON.parse(readFileSync("ARCHITECTURE_RULES.json", "utf8"));
const supported = new Set(["frontend_process_ownership", "system_python_fallback", "socket_bind_address_policy", "whole_app_elevation", "self_update_absence", "telemetry_absence", "visual_dependency_absence", "group_review_gate"]);
if (!Array.isArray(rules.rules) || rules.rules.length !== 24) throw new Error("architecture rule inventory must contain 24 rules");
const enforced = rules.rules.filter((r) => r.verification?.mode === "enforced");
const deferred = rules.rules.filter((r) => r.verification?.mode === "deferred");
if (enforced.length !== 8 || deferred.length !== 16) throw new Error(`architecture evidence counts mismatch enforced=${enforced.length} deferred=${deferred.length}`);
for (const rule of rules.rules) {
  const v = rule.verification;
  if (!v) throw new Error(`${rule.id} missing verification declaration`);
  if (v.mode === "enforced" && !supported.has(v.type)) throw new Error(`${rule.id} unsupported verifier type`);
  if (v.mode === "deferred" && (!/^LB-\d{3}$/.test(v.activate_at_pr ?? "") || !v.reason)) throw new Error(`${rule.id} invalid deferred verification declaration`);
  if (!enforced.includes(rule) && !deferred.includes(rule)) throw new Error(`${rule.id} unsupported verification mode`);
}

const architectureRunner = readFileSync("scripts/verify-architecture/index.mjs", "utf8");
if (architectureRunner.includes("rules=${rules.rules.length}")) throw new Error("architecture runner still contains misleading all-rules PASS output");
if (!architectureRunner.includes("enforced=${enforced.length} deferred=${deferred.length} total=${rulesDoc.rules.length}")) throw new Error("architecture runner honest evidence output missing");
if (!pkg.scripts?.["verify:architecture:negative"]?.includes("--expected ARCH-001,ARCH-003,ARCH-023,ARCH-024")) throw new Error("architecture negative fixture does not require exact rule IDs");

const packagingSmoke = readFileSync("scripts/lb001-packaging-smoke.mjs", "utf8");
for (const required of ["@tauri-apps", "tauri.js", '"build", "--bundles", "nsis"', "lb001-nsis-install", "installedSidecarSha", "LB001_REAL_TAURI_NSIS_SMOKE=PASS"]) {
  if (!packagingSmoke.includes(required)) throw new Error(`real Tauri NSIS smoke evidence missing: ${required}`);
}
if (/\bcpSync\b|lb001-release-bundle/.test(packagingSmoke)) throw new Error("manual staging packaging smoke is forbidden");
console.log("LB001_CONTRACT_VERIFY=PASS");
