import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

for (const [cmd, args] of [
  ["npm", ["run", "build"]],
  ["cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--release", "--bin", "localbridge", "--locked"]],
  ["node", ["scripts/prepare-dummy-sidecar.mjs"]],
]) {
  const result = spawnSync(cmd, args, { stdio: "inherit", shell: process.platform === "win32" });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

const bundle = join("tests", "artifacts", "lb001-release-bundle");
rmSync(bundle, { recursive: true, force: true });
mkdirSync(bundle, { recursive: true });
const appExe = join("src-tauri", "target", "release", "localbridge.exe");
const sidecar = join("src-tauri", "binaries", "dummy-sidecar-x86_64-pc-windows-msvc.exe");
const icon = join("assets", "icons", "localbridge.ico");
for (const file of [appExe, sidecar, icon]) {
  if (!existsSync(file)) throw new Error(`packaging input missing: ${file}`);
}
cpSync(appExe, join(bundle, "LocalBridge.exe"));
cpSync(sidecar, join(bundle, "dummy-sidecar.exe"));
cpSync(icon, join(bundle, "localbridge.ico"));
const run = spawnSync(join(bundle, "dummy-sidecar.exe"), [], { encoding: "utf8" });
if (run.status !== 0 || run.stdout.trim() !== "LOCALBRIDGE_DUMMY_SIDECAR_OK") {
  throw new Error("packaged dummy sidecar did not launch correctly");
}
console.log("LB001_RELEASE_BUNDLE_SMOKE=PASS");
