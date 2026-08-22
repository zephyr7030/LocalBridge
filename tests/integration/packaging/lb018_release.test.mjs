import { readFileSync, existsSync } from "node:fs";

const tauri = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const manifest = readFileSync("runtime-manifest.toml", "utf8");
const buildRs = readFileSync("src-tauri/build.rs", "utf8");
const broker = readFileSync("src-tauri/bin/privileged-broker/main.rs", "utf8");
const prep = readFileSync("scripts/prepare-lb018-resources.mjs", "utf8");
const release = readFileSync("scripts/lb018-release.mjs", "utf8");
const nsisHooks = readFileSync("scripts/public-release/nsis-hooks.nsh", "utf8");
const credentials = readFileSync("src-tauri/src/credentials/mod.rs", "utf8");
const windowsCredentials = readFileSync("src-tauri/src/credentials/windows.rs", "utf8");

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
if (tauri.bundle?.windows?.nsis?.installerHooks !== "../scripts/public-release/nsis-hooks.nsh") throw new Error("NSIS uninstall credential hook is not wired");
if (!credentials.includes('RUNTIME_API_KEY_CREDENTIAL_ID: &str = "runtime-api-key"')) throw new Error("runtime API key credential id drifted");
if (!windowsCredentials.includes('TARGET_PREFIX: &str = "LocalBridge/RuntimeApiKey/"')) throw new Error("runtime API key credential prefix drifted");
for (const marker of ["NSIS_HOOK_PREUNINSTALL", "MB_DEFBUTTON2", "/DELETEUSERDATA=1", "CredDeleteW", "LocalBridge/RuntimeApiKey/runtime-api-key", "i 1", "i 0"]) if (!nsisHooks.includes(marker)) throw new Error(`NSIS credential cleanup marker missing: ${marker}`);
if (/cmdkey|powershell|execwait/i.test(nsisHooks)) throw new Error("NSIS credential cleanup must not spawn a shell or helper process");
if (tauri.bundle?.icon?.[0] !== "../assets/icons/localbridge.ico") throw new Error("frozen installer icon drifted");
if (tauri.build?.beforeBuildCommand !== "node scripts/prepare-lb018-resources.mjs && npm run build") throw new Error("release resource preparation not wired before build");
if (/dummy-sidecar/i.test(buildRs) || /dummy-sidecar/i.test(JSON.stringify(tauri))) throw new Error("dummy packaging remains active");
if (!broker.startsWith("#![cfg_attr(windows, windows_subsystem = \"windows\")]")) throw new Error("broker is not a Windows subsystem binary");
if (/cloudflared|cloudflare managed/i.test(manifest)) throw new Error("final runtime manifest advertises Cloudflare runtime");
if (existsSync("runtime/tunnel-client/cloudflared.exe") || existsSync("runtime/tunnel-client/cloudflared-manifest.json")) throw new Error("Cloudflare runtime remains in final payload");
for (const marker of ["prepare-toolbox.mjs", "localbridge-privileged-broker", "cargo", "release-stage"]) if (!prep.includes(marker)) throw new Error(`release prepare marker missing: ${marker}`);
for (const marker of [
  "CycloneDX", "package-inventory.json", "release-provenance.json", "size-report.json",
  "LocalBridge.exe", "runtime-manifest.toml", "runtime-policy.toml", "THIRD_PARTY_NOTICES.md",
  "inventory hash mismatch", "installer hash mismatch", "artifact installer does not match current NSIS build output",
  "LB019PRE_RELEASE_${label}=PASS", "buildReleaseTransaction(\"REFRESH\")", "build_binding",
  "assertTrackedSourceClean", "src-tauri/target/release/bundle/nsis", "src-tauri/target/release/localbridge.exe",
  "configured_foreground_runtime_start", "background_launch", "runtime_restart_or_recovery",
  "tunnel_reconnect", "login_autostart", "managed_shell_or_direct_command_child",
  "CREATE_NO_WINDOW", "cloudflared", "verifyUninstallCredentialCleanupInvariant",
  "uninstall_preserves_user_data_by_default", "uninstall_deletes_runtime_api_key_only_with_explicit_consent"
]) if (!release.includes(marker)) throw new Error(`release evidence marker missing: ${marker}`);
if (release.includes("coding_runtime_managed_command_visible_window_behavior_gate")) throw new Error("obsolete single-scenario no-console evidence remains");
console.log("LB018_PACKAGING_CONTRACT=PASS real_runtime=true dummy=false cloudflared=false broker=true sbom=true provenance=true credential_retention_default=true explicit_cleanup=true");
