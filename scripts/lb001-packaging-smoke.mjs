import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { basename, join, resolve } from "node:path";

if (process.platform !== "win32" || process.arch !== "x64") {
  throw new Error("LB-001 NSIS packaging smoke requires Windows x64");
}

const run = (command, args, options = {}) => {
  const result = spawnSync(command, args, { stdio: "inherit", windowsHide: true, ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${basename(command)} failed with exit ${result.status}`);
  return result;
};
const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const walk = (dir) => {
  if (!existsSync(dir)) return [];
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const stat = statSync(path);
    if (stat.isDirectory()) out.push(...walk(path));
    else out.push(path);
  }
  return out;
};

const tauriCli = resolve("node_modules", "@tauri-apps", "cli", "tauri.js");
if (!existsSync(tauriCli)) throw new Error("local @tauri-apps/cli entrypoint is missing; implicit package install is forbidden");
run(process.execPath, [resolve("scripts", "prepare-dummy-sidecar.mjs")]);

const sourceSidecar = resolve("src-tauri", "binaries", "dummy-sidecar-x86_64-pc-windows-msvc.exe");
if (!existsSync(sourceSidecar)) throw new Error("prepared dummy sidecar is missing");
const sourceSidecarSha = sha256(sourceSidecar);

const nsisDir = resolve("src-tauri", "target", "release", "bundle", "nsis");
rmSync(nsisDir, { recursive: true, force: true });
rmSync(resolve("src-tauri", "target", "release", "dummy-sidecar.exe"), { force: true });
rmSync(resolve("src-tauri", "target", "release", "dummy-sidecar.pdb"), { force: true });
run(process.execPath, [tauriCli, "build", "--bundles", "nsis"]);

const installers = walk(nsisDir).filter((path) => /\.exe$/i.test(path));
if (installers.length !== 1) throw new Error(`expected exactly one fresh NSIS installer, found ${installers.length}`);
const installer = installers[0];
const installerStat = statSync(installer);
if (installerStat.size <= 0) throw new Error("generated NSIS installer is empty");

const installDir = resolve("tests", "artifacts", "lb001-nsis-install");
rmSync(installDir, { recursive: true, force: true });
run(installer, ["/S", `/D=${installDir}`]);
let installedSidecarSha;
let verificationError;
let cleanupError;
try {
  const installedFiles = walk(installDir);
  const installedApp = installedFiles.find((path) => /^localbridge\.exe$/i.test(basename(path)));
  const installedSidecars = installedFiles.filter((path) => /^dummy-sidecar(?:-[^.]+)?\.exe$/i.test(basename(path)));
  if (!installedApp) throw new Error("real NSIS install did not contain LocalBridge.exe");
  if (installedSidecars.length !== 1) throw new Error(`real NSIS install expected one dummy sidecar, found ${installedSidecars.length}`);
  const installedSidecar = installedSidecars[0];
  installedSidecarSha = sha256(installedSidecar);
  if (installedSidecarSha !== sourceSidecarSha) throw new Error("installed sidecar SHA256 differs from prepared externalBin");

  const sidecarRun = spawnSync(installedSidecar, [], { encoding: "utf8", windowsHide: true });
  if (sidecarRun.error) throw sidecarRun.error;
  if (sidecarRun.status !== 0 || sidecarRun.stdout.trim() !== "LOCALBRIDGE_DUMMY_SIDECAR_OK") {
    throw new Error("dummy sidecar from the real NSIS install did not execute successfully");
  }

  const webViewPayload = installedFiles.find((path) => /(?:microsoftedge)?webview2|webview2loader/i.test(path));
  if (webViewPayload) throw new Error(`bundled WebView2 payload detected in installed tree: ${basename(webViewPayload)}`);
} catch (error) {
  verificationError = error;
} finally {
  const uninstaller = walk(installDir).find((path) => /uninstall.*\.exe$/i.test(basename(path)));
  if (uninstaller) {
    const uninstall = spawnSync(uninstaller, ["/S"], { stdio: "inherit", windowsHide: true });
    if (uninstall.error) cleanupError = uninstall.error;
    else if (uninstall.status !== 0) cleanupError = new Error(`NSIS uninstaller failed with exit ${uninstall.status}`);
  }
  rmSync(installDir, { recursive: true, force: true });
}
if (verificationError) throw verificationError;
if (cleanupError) throw cleanupError;

console.log(`LB001_REAL_TAURI_NSIS_SMOKE=PASS installer=${basename(installer)} installer_bytes=${installerStat.size} sidecar_sha256=${installedSidecarSha} webview2_payload=false`);
