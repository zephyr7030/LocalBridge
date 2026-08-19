import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const expected = new Map([
  ["runtime/tunnel-client/tunnel-client.exe", "7d3c7d492ce84b52835e11865a835a8a5bcd4a669dee84e169aa11b314dc952a"],
  ["runtime/tunnel-client/LICENSE", "f4c1d7ba32ef5bcf5cf03e2eefec5825ebafedf50fa330a36700a49c605c1ef4"],
]);
for (const [path, hash] of expected) {
  const actual = sha256(path);
  if (actual !== hash) throw new Error(`LB-008 payload SHA256 mismatch: ${path} ${actual}`);
}

for (const forbidden of ["runtime/tunnel-client/cloudflared.exe", "runtime/tunnel-client/cloudflared-manifest.json"]) {
  if (existsSync(forbidden)) throw new Error(`LB-018 retired runtime payload still exists: ${forbidden}`);
}

const runHelp = readFileSync("compatibility/tunnel-client/0.0.11/run-help.txt", "utf8");
const runtimeSource = readFileSync("src-tauri/src/tunnel/runtime.rs", "utf8");
const managedHelpLine = runHelp
  .split(/\r?\n/)
  .find((line) => line.includes("CLOUDFLARED_MANAGED") && line.includes("(optional)"));
if (!managedHelpLine) {
  throw new Error("LB-008 pinned 0.0.11 help no longer proves managed Cloudflare is optional");
}
const quickstartLine = runHelp
  .split(/\r?\n/)
  .find((line) => line.includes("tunnel-client run --embedded-mcp-stub"));
if (!quickstartLine) throw new Error("LB-008 pinned 0.0.11 quickstart line missing");
const managedFlags = ["--cloudflared.managed", "--cloudflared.path", "--cloudflared.token"];
for (const forbidden of managedFlags) {
  if (quickstartLine.includes(forbidden)) {
    throw new Error(`LB-008 pinned ordinary quickstart unexpectedly requires ${forbidden}`);
  }
}
const argvStart = runtimeSource.indexOf("pub fn command_line_arguments");
const argvEnd = runtimeSource.indexOf("pub fn health_url_file", argvStart);
if (argvStart < 0 || argvEnd <= argvStart) throw new Error("LB-008 could not isolate ordinary tunnel argv builder");
const argvBuilder = runtimeSource.slice(argvStart, argvEnd);
for (const forbidden of managedFlags) {
  if (argvBuilder.includes(forbidden)) {
    throw new Error(`LB-008 ordinary Tunnel startup must not force optional managed Cloudflare flag ${forbidden}`);
  }
}
for (const removedEnv of ["CLOUDFLARED_TUNNEL_TOKEN", "CLOUDFLARED_MANAGED", "CLOUDFLARED_PATH", "CLOUDFLARED_READY_TIMEOUT"]) {
  if (!runtimeSource.includes(`\"${removedEnv}\"`)) throw new Error(`LB-018 must defensively remove inherited ${removedEnv}`);
  if (argvBuilder.includes(removedEnv)) throw new Error(`LB-018 ordinary Tunnel argv must not activate ${removedEnv}`);
}

const tunnelVersion = spawnSync("runtime/tunnel-client/tunnel-client.exe", ["--version"], {
  encoding: "utf8",
  windowsHide: true,
});
if (tunnelVersion.status !== 0) throw new Error(`LB-008 tunnel-client --version failed: ${tunnelVersion.stderr}`);
if (tunnelVersion.stdout.trim() !== "0.0.11+8d55683eeef80bc5e360d95abf4692454fafc615 (git sha: 8d55683eeef80bc5e360d95abf4692454fafc615)") {
  throw new Error(`LB-008 tunnel-client identity mismatch: ${tunnelVersion.stdout.trim()}`);
}

const manifest = readFileSync("runtime-manifest.toml", "utf8");
for (const line of [
  'version = "0.0.11"',
  'executable = "runtime/tunnel-client/tunnel-client.exe"',
  'executable_sha256 = "7d3c7d492ce84b52835e11865a835a8a5bcd4a669dee84e169aa11b314dc952a"',
  'license = "runtime/tunnel-client/LICENSE"',
  'license_sha256 = "f4c1d7ba32ef5bcf5cf03e2eefec5825ebafedf50fa330a36700a49c605c1ef4"',
  'api_key_reference = "env:LOCALBRIDGE_RUNTIME_API_KEY"',
  'api_key_injection = "child_environment:LOCALBRIDGE_RUNTIME_API_KEY"',
  'tunnel_id_injection = "child_environment:CONTROL_PLANE_TUNNEL_ID"',
  'production_control_plane = "https://api.openai.com"',
  'health_bind = "127.0.0.1:0"',
  'ambient_parent_overrides = "removed_before_spawn"',
  'stable_adapter = "src-tauri/src/tunnel"',
]) {
  if (!manifest.includes(line)) throw new Error(`LB-008 runtime manifest contract missing: ${line}`);
}
if (/cloudflared|cloudflare managed/i.test(manifest)) throw new Error("LB-018 final runtime manifest still advertises Cloudflare runtime");

const historicalAsset = JSON.parse(readFileSync("compatibility/tunnel-client/0.0.11/release-asset.json", "utf8"));
if (!historicalAsset.contents?.some((item) => item.name === "cloudflared.exe")) {
  throw new Error("LB-018 historical compatibility evidence was unexpectedly rewritten");
}

console.log("LB008_TUNNEL_CONTRACT=PASS tunnel=0.0.11 standalone_runtime=true cloudflared=false exact_hashes=true env_only_api_key=true historical_cloudflare_non_executable=true");
