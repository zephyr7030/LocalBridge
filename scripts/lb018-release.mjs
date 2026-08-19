import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(import.meta.dirname, "..");
const artifacts = resolve(root, "release-artifacts/LB-019PRE");
const PRODUCT_VERSION = "0.1.1";
const sha = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const run = (program, args, options = {}) => {
  const result = spawnSync(program, args, { cwd: options.cwd ?? root, encoding: "utf8", stdio: options.stdio ?? "pipe", windowsHide: true, maxBuffer: 256 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`${program} ${args.join(" ")} failed (${result.status ?? "start failure"}): ${String(result.stderr ?? result.error?.message ?? "").trim()}`);
  return String(result.stdout ?? "");
};
const walk = (base, dir = base, out = []) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name); const info = statSync(path);
    if (info.isDirectory()) walk(base, path, out);
    else if (info.isFile()) out.push({ path: relative(base, path).replaceAll("\\", "/"), bytes: info.size, sha256: sha(path) });
  }
  return out;
};
const peSubsystem = (path) => {
  const data = readFileSync(path); if (data.toString("ascii", 0, 2) !== "MZ") throw new Error(`not PE: ${path}`);
  const pe = data.readUInt32LE(0x3c); if (data.toString("ascii", pe, pe + 4) !== "PE\0\0") throw new Error(`bad PE: ${path}`);
  const optional = pe + 24; return data.readUInt16LE(optional + 68);
};
const json = (name, value) => writeFileSync(resolve(artifacts, name), `${JSON.stringify(value, null, 2)}\n`, "utf8");
const forbiddenPath = (value) => /(?:^|\/)(?:tests?|source-tree|governance|skills|templates|\.coding-tools)(?:\/|$)|cloudflared|PR_CONTRACTS|PROJECT_STATE|PR_INDEX|FINAL_REVIEW|\.dmp$|\.log$/i.test(value);

export function primaryManagedSpawnUsesNoWindow(source) {
  const start = source.indexOf("pub fn spawn(spec: &ManagedProcessSpec)");
  if (start < 0) return false;
  const end = source.indexOf("pub const fn snapshot", start);
  const body = source.slice(start, end < 0 ? source.length : end);
  const assignment = body.match(/let\s+creation_flags\s*=([\s\S]*?);/);
  if (!assignment) return false;
  const flags = assignment[1];
  return flags.includes("CREATE_SUSPENDED")
    && flags.includes("CREATE_NO_WINDOW")
    && flags.includes("CREATE_UNICODE_ENVIRONMENT")
    && /CreateProcessW\([\s\S]*?creation_flags\s*,/.test(body);
}

function verifyReleaseVersionSurfaces() {
  const packageJson = JSON.parse(readFileSync(resolve(root, "package.json"), "utf8"));
  const packageLock = JSON.parse(readFileSync(resolve(root, "package-lock.json"), "utf8"));
  const tauri = JSON.parse(readFileSync(resolve(root, "src-tauri/tauri.conf.json"), "utf8"));
  const cargoToml = readFileSync(resolve(root, "src-tauri/Cargo.toml"), "utf8");
  const cargoLock = readFileSync(resolve(root, "src-tauri/Cargo.lock"), "utf8");
  const runtimeManifest = readFileSync(resolve(root, "runtime-manifest.toml"), "utf8");
  const localbridgeLock = cargoLock.match(/\[\[package\]\][\s\S]*?name = "localbridge"\s*\nversion = "([^"]+)"/);
  const versions = [
    packageJson.version,
    packageLock.version,
    packageLock.packages?.[""]?.version,
    tauri.version,
    cargoToml.match(/^version = "([^"]+)"/m)?.[1],
    localbridgeLock?.[1],
    runtimeManifest.match(/^release = "([^"]+)"/m)?.[1],
  ];
  if (versions.some((version) => version !== PRODUCT_VERSION)) {
    throw new Error(`release version surfaces disagree: ${JSON.stringify(versions)}`);
  }
}

function verifyNoVisibleConsoleBehavior() {
  run("cargo", [
    "test",
    "--manifest-path", "src-tauri/Cargo.toml",
    "--test", "lb006_coding_runtime",
    "actual_bundled_runtime_is_authenticated_loopback_owned_and_secret_redacted",
    "--", "--nocapture", "--test-threads=1",
  ], { stdio: "inherit" });
  return true;
}

function generateSbom() {
  const npm = JSON.parse(readFileSync(resolve(root, "package-lock.json"), "utf8"));
  const cargo = JSON.parse(run("cargo", ["metadata", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--format-version", "1"]));
  const components = [];
  for (const [path, pkg] of Object.entries(npm.packages ?? {})) if (path) components.push({ type: "library", name: basename(path), version: pkg.version ?? "unknown", licenses: pkg.license ? [{ license: { id: pkg.license } }] : undefined, properties: [{ name: "localbridge:ecosystem", value: "npm" }] });
  for (const pkg of cargo.packages) components.push({ type: "library", name: pkg.name, version: pkg.version, licenses: pkg.license ? [{ expression: pkg.license }] : undefined, properties: [{ name: "localbridge:ecosystem", value: "cargo" }] });
  for (const component of [
    ["CPython Embedded", "3.12.10"], ["coding-tools-mcp", "0.2.2"], ["OpenAI tunnel-client", "0.0.11"], ["aria2c", "1.37.0"], ["7-Zip", "26.02"], ["jq", "1.8.2"], ["Windows System curl", "system"], ["Microsoft WebView2 Runtime", "system"],
  ]) components.push({ type: "application", name: component[0], version: component[1], properties: [{ name: "localbridge:runtime", value: "true" }] });
  json("sbom.cdx.json", { bomFormat: "CycloneDX", specVersion: "1.5", version: 1, metadata: { component: { type: "application", name: "LocalBridge", version: PRODUCT_VERSION } }, components });
}

function findInstaller() {
  const dir = resolve(root, "src-tauri/target/release/bundle/nsis");
  if (!existsSync(dir)) throw new Error("NSIS output directory missing");
  const candidates = readdirSync(dir).filter((name) => name.toLowerCase().endsWith(".exe")).map((name) => resolve(dir, name));
  if (candidates.length !== 1) throw new Error(`expected one NSIS installer, found ${candidates.length}`);
  return candidates[0];
}

function emitEvidence(codingRuntimeNoVisibleConsole) {
  verifyReleaseVersionSurfaces();
  if (codingRuntimeNoVisibleConsole !== true) throw new Error("coding runtime no-visible-console behavior gate missing");
  mkdirSync(artifacts, { recursive: true });
  const installer = findInstaller();
  const targetInstaller = resolve(artifacts, basename(installer));
  writeFileSync(targetInstaller, readFileSync(installer));
  generateSbom();
  const releaseFiles = [
    { path: "LocalBridge.exe", bytes: statSync(resolve(root, "src-tauri/target/release/localbridge.exe")).size, sha256: sha(resolve(root, "src-tauri/target/release/localbridge.exe")) },
    { path: "localbridge-privileged-broker.exe", bytes: statSync(resolve(root, "src-tauri/target/release-stage/localbridge-privileged-broker.exe")).size, sha256: sha(resolve(root, "src-tauri/target/release-stage/localbridge-privileged-broker.exe")) },
    ...["runtime-manifest.toml", "runtime-policy.toml", "LICENSE", "THIRD_PARTY_NOTICES.md"].map((path) => ({ path, bytes: statSync(resolve(root, path)).size, sha256: sha(resolve(root, path)) })),
    ...walk(resolve(root, "runtime/python")).map((x) => ({ ...x, path: `runtime/python/${x.path}` })),
    ...walk(resolve(root, "runtime/coding-tools-mcp")).map((x) => ({ ...x, path: `runtime/coding-tools-mcp/${x.path}` })),
    ...walk(resolve(root, "runtime/tunnel-client")).map((x) => ({ ...x, path: `runtime/tunnel-client/${x.path}` })),
    ...walk(resolve(root, "src-tauri/target/toolbox-stage")).map((x) => ({ ...x, path: `runtime/toolbox/${x.path.replace(/^bin\//, "bin/")}` })),
  ];
  for (const file of releaseFiles) if (forbiddenPath(file.path)) throw new Error(`forbidden release payload: ${file.path}`);
  const broker = resolve(root, "src-tauri/target/release-stage/localbridge-privileged-broker.exe");
  const main = resolve(root, "src-tauri/target/release/localbridge.exe");
  const inventory = { schema: 1, source_commit: run("git", ["rev-parse", "HEAD"]).trim(), payload_files: releaseFiles, installer: { file: basename(targetInstaller), bytes: statSync(targetInstaller).size, sha256: sha(targetInstaller) } };
  json("package-inventory.json", inventory);
  const installedBytes = releaseFiles.reduce((sum, item) => sum + item.bytes, 0);
  const size = { installer_bytes: statSync(targetInstaller).size, installer_mib: Number((statSync(targetInstaller).size / 1048576).toFixed(2)), installed_payload_bytes: installedBytes, installed_payload_mib: Number((installedBytes / 1048576).toFixed(2)) };
  json("size-report.json", size);
  const mainSubsystem = peSubsystem(main), brokerSubsystem = peSubsystem(broker);
  if (mainSubsystem !== 2 || brokerSubsystem !== 2) throw new Error(`release GUI subsystem mismatch main=${mainSubsystem} broker=${brokerSubsystem}`);
  json("release-provenance.json", {
    schema: 1, product: "LocalBridge", version: PRODUCT_VERSION, target: "windows-11-x86_64", source_commit: inventory.source_commit,
    installer: inventory.installer, runtime_manifest_sha256: sha(resolve(root, "runtime-manifest.toml")), sbom_sha256: sha(resolve(artifacts, "sbom.cdx.json")),
    packaging: { per_machine: true, system_webview2: true, bundled_webview2: false, cloudflared: false, runtime_payload_location: "install-root/runtime", mutable_state_root: "%LOCALAPPDATA%\\LocalBridge", secret_store: "Windows Credential Manager" },
    no_console_evidence: { localbridge_pe_subsystem: mainSubsystem, broker_pe_subsystem: brokerSubsystem, managed_runtime_supervisor_uses_CREATE_NO_WINDOW: primaryManagedSpawnUsesNoWindow(readFileSync(resolve(root, "src-tauri/src/runtime/windows_supervisor.rs"), "utf8")), privileged_execution_uses_CREATE_NO_WINDOW: readFileSync(resolve(root, "src-tauri/src/privilege/execution.rs"), "utf8").includes("CREATE_NO_WINDOW"), coding_runtime_managed_command_visible_window_behavior_gate: codingRuntimeNoVisibleConsole },
    ordinary_launch: { token: "current_windows_user", integrity: "medium", foreground_uac: false, background_uac: false, login_autostart_uac: false, high_integrity_route: "elevated_exec_broker_uac_only" }
  });
  return { installer: basename(targetInstaller), size };
}

function build() {
  const cli = resolve(root, "node_modules/@tauri-apps/cli/tauri.js");
  if (!existsSync(cli)) throw new Error("local Tauri CLI missing; run npm ci");
  const codingRuntimeNoVisibleConsole = verifyNoVisibleConsoleBehavior();
  run(process.execPath, [cli, "build", "--bundles", "nsis"], { stdio: "inherit" });
  const result = emitEvidence(codingRuntimeNoVisibleConsole);
  console.log(`LB019PRE_RELEASE_BUILD=PASS installer=${result.installer} size_mib=${result.size.installer_mib} installed_payload_mib=${result.size.installed_payload_mib}`);
}

function refresh() {
  const codingRuntimeNoVisibleConsole = verifyNoVisibleConsoleBehavior();
  const result = emitEvidence(codingRuntimeNoVisibleConsole);
  console.log(`LB019PRE_RELEASE_REFRESH=PASS installer=${result.installer} source_commit=${run("git", ["rev-parse", "HEAD"]).trim()}`);
}

function verify() {
  verifyReleaseVersionSurfaces();
  const codingRuntimeNoVisibleConsole = verifyNoVisibleConsoleBehavior();
  for (const path of ["sbom.cdx.json", "package-inventory.json", "release-provenance.json", "size-report.json"]) if (!existsSync(resolve(artifacts, path))) throw new Error(`release evidence missing: ${path}`);
  const inventory = JSON.parse(readFileSync(resolve(artifacts, "package-inventory.json"), "utf8"));
  const requiredPayloads = ["LocalBridge.exe", "localbridge-privileged-broker.exe", "runtime-manifest.toml", "runtime-policy.toml", "LICENSE", "THIRD_PARTY_NOTICES.md"];
  for (const path of requiredPayloads) if (!inventory.payload_files.some((item) => item.path === path)) throw new Error(`required inventory payload missing: ${path}`);
  const seen = new Set();
  for (const item of inventory.payload_files) if (forbiddenPath(item.path)) throw new Error(`forbidden inventory path: ${item.path}`);
  for (const item of inventory.payload_files) {
    if (seen.has(item.path)) throw new Error(`duplicate inventory path: ${item.path}`);
    seen.add(item.path);
    const local = item.path === "LocalBridge.exe" ? resolve(root, "src-tauri/target/release/localbridge.exe")
      : item.path === "localbridge-privileged-broker.exe" ? resolve(root, "src-tauri/target/release-stage/localbridge-privileged-broker.exe")
      : item.path.startsWith("runtime/toolbox/") ? resolve(root, "src-tauri/target/toolbox-stage", item.path.slice("runtime/toolbox/".length))
      : resolve(root, item.path);
    if (!existsSync(local) || statSync(local).size !== item.bytes || sha(local) !== item.sha256) throw new Error(`inventory hash mismatch: ${item.path}`);
  }
  if (JSON.stringify(inventory).toLowerCase().includes("cloudflared")) throw new Error("cloudflared present in release inventory");
  const installer = resolve(artifacts, inventory.installer.file);
  if (!existsSync(installer) || statSync(installer).size !== inventory.installer.bytes || sha(installer) !== inventory.installer.sha256) throw new Error("installer hash mismatch");
  const provenance = JSON.parse(readFileSync(resolve(artifacts, "release-provenance.json"), "utf8"));
  if (provenance.source_commit !== inventory.source_commit) throw new Error("release provenance source commit mismatch");
  if (provenance.packaging.cloudflared !== false || provenance.packaging.bundled_webview2 !== false) throw new Error("release provenance packaging invariant failed");
  if (!Object.values(provenance.no_console_evidence).every((value) => value === 2 || value === true)) throw new Error("no-console evidence incomplete");
  if (provenance.no_console_evidence.coding_runtime_managed_command_visible_window_behavior_gate !== codingRuntimeNoVisibleConsole) throw new Error("coding runtime no-visible-console behavior evidence mismatch");
  if (!primaryManagedSpawnUsesNoWindow(readFileSync(resolve(root, "src-tauri/src/runtime/windows_supervisor.rs"), "utf8"))) throw new Error("primary managed runtime spawn is missing CREATE_NO_WINDOW");
  console.log("LB019PRE_RELEASE_VERIFY=PASS cloudflared=false webview2=system sbom=true provenance=true no_console=true");
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const command = process.argv[2] ?? "build";
  if (command === "build") build(); else if (command === "refresh") refresh(); else if (command === "verify") verify(); else throw new Error("usage: node scripts/lb018-release.mjs <build|refresh|verify>");
}
