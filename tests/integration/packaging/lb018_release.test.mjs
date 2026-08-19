import { readFileSync, existsSync } from "node:fs";

const tauri = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const manifest = readFileSync("runtime-manifest.toml", "utf8");
const buildRs = readFileSync("src-tauri/build.rs", "utf8");
const broker = readFileSync("src-tauri/bin/privileged-broker/main.rs", "utf8");
const prep = readFileSync("scripts/prepare-lb018-resources.mjs", "utf8");
const release = readFileSync("scripts/lb018-release.mjs", "utf8");

if (tauri.bundle?.externalBin?.length) throw new Error("LB-001 dummy externalBin remains in final package");
for (const [source, target] of Object.entries({
  "../runtime/python/": "runtime/python/",
  "../runtime/coding-tools-mcp/": "runtime/coding-tools-mcp/",
  "../runtime/tunnel-client/": "runtime/tunnel-client/",
  "target/toolbox-stage/": "runtime/toolbox/",
  "target/release-stage/localbridge-privileged-broker.exe": "localbridge-privileged-broker.exe",
  "../runtime-manifest.toml": "runtime-manifest.toml",
  "../runtime-policy.toml": "runtime-policy.toml",
  "../LICENSE": "LICENSE",
  "../THIRD_PARTY_NOTICES.md": "THIRD_PARTY_NOTICES.md",
})) if (tauri.bundle?.resources?.[source] !== target) throw new Error(`missing release resource mapping ${source} -> ${target}`);
if (tauri.bundle?.windows?.nsis?.installMode !== "perMachine") throw new Error("installer must remain perMachine");
if (tauri.bundle?.icon?.[0] !== "../assets/icons/localbridge.ico") throw new Error("frozen installer icon drifted");
if (tauri.build?.beforeBuildCommand !== "node scripts/prepare-lb018-resources.mjs && npm run build") throw new Error("release resource preparation not wired before build");
if (/dummy-sidecar/i.test(buildRs) || /dummy-sidecar/i.test(JSON.stringify(tauri))) throw new Error("dummy packaging remains active");
if (!broker.startsWith("#![cfg_attr(windows, windows_subsystem = \"windows\")]")) throw new Error("broker is not a Windows subsystem binary");
if (/cloudflared|cloudflare managed/i.test(manifest)) throw new Error("final runtime manifest advertises Cloudflare runtime");
if (existsSync("runtime/tunnel-client/cloudflared.exe") || existsSync("runtime/tunnel-client/cloudflared-manifest.json")) throw new Error("Cloudflare runtime remains in final payload");
for (const marker of ["prepare-toolbox.mjs", "localbridge-privileged-broker", "cargo", "release-stage"]) if (!prep.includes(marker)) throw new Error(`release prepare marker missing: ${marker}`);
for (const marker of ["CycloneDX", "package-inventory.json", "release-provenance.json", "size-report.json", "LocalBridge.exe", "runtime-manifest.toml", "runtime-policy.toml", "THIRD_PARTY_NOTICES.md", "inventory hash mismatch", "installer hash mismatch", "LB018_RELEASE_REFRESH=PASS", "CREATE_NO_WINDOW", "cloudflared"]) if (!release.includes(marker)) throw new Error(`release evidence marker missing: ${marker}`);
console.log("LB018_PACKAGING_CONTRACT=PASS real_runtime=true dummy=false cloudflared=false broker=true sbom=true provenance=true");
