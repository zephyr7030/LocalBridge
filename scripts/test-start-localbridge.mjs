import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";

const launcher = readFileSync("start-localbridge.cmd", "utf8");
const viteConfig = readFileSync("vite.config.ts", "utf8");
const tauriConfig = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));

assert.equal(tauriConfig.build?.devUrl, "http://127.0.0.1:1420", "Tauri devUrl drifted from the frozen loopback development endpoint");
for (const required of [
  'host: "127.0.0.1"',
  "port: 1420",
  "strictPort: true",
]) assert.ok(viteConfig.includes(required), `Vite/Tauri development endpoint mismatch: ${required}`);

for (const required of [
  'cd /d "%~dp0"',
  '"--check"',
  'node_modules\\.bin\\tauri.cmd" dev',
  'LOCALBRIDGE_LAUNCHER_CHECK=PASS',
  'runtime\\python\\python.exe',
  'runtime\\coding-tools-mcp\\coding_tools_mcp\\__init__.py',
  'runtime\\tunnel-client\\tunnel-client.exe',
]) assert.ok(launcher.includes(required), `launcher contract missing: ${required}`);

for (const forbidden of [
  /powershell/i,
  /npm\s+(?:ci|install)/i,
  /curl|wget/i,
  /runtime[_ -]?api[_ -]?key/i,
  /credential/i,
  /secret/i,
]) assert.ok(!forbidden.test(launcher), `launcher contains forbidden behavior: ${forbidden}`);

const run = (arg) => spawnSync("cmd.exe", ["/d", "/s", "/c", `start-localbridge.cmd ${arg}`], {
  cwd: process.cwd(),
  encoding: "utf8",
  windowsHide: true,
});

const check = run("--check");
assert.equal(check.status, 0, `launcher --check failed: ${check.stdout}\n${check.stderr}`);
assert.match(check.stdout, /LOCALBRIDGE_LAUNCHER_CHECK=PASS/);

const printed = run("--print-command");
assert.equal(printed.status, 0, `launcher --print-command failed: ${printed.stdout}\n${printed.stderr}`);
assert.match(printed.stdout, /node_modules\\\.bin\\tauri\.cmd dev/);

const dev = spawn("cmd.exe", ["/d", "/s", "/c", "npm run dev"], {
  cwd: process.cwd(),
  windowsHide: true,
  stdio: ["ignore", "pipe", "pipe"],
});
let devOutput = "";
dev.stdout.on("data", (chunk) => { devOutput += chunk.toString(); });
dev.stderr.on("data", (chunk) => { devOutput += chunk.toString(); });

let frontendReady = false;
try {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (dev.exitCode !== null) break;
    try {
      const response = await fetch("http://127.0.0.1:1420/", { signal: AbortSignal.timeout(500) });
      if (response.ok) {
        frontendReady = true;
        break;
      }
    } catch {
      // Expected until Vite is listening.
    }
    await delay(100);
  }
} finally {
  if (dev.pid) {
    spawnSync("taskkill", ["/pid", String(dev.pid), "/t", "/f"], {
      encoding: "utf8",
      windowsHide: true,
    });
  }
}
assert.ok(frontendReady, `Vite did not become ready at Tauri devUrl http://127.0.0.1:1420:\n${devOutput}`);

console.log("LOCALBRIDGE_LAUNCHER_TEST=PASS offline_check=true no_powershell=true no_install=true exact_tauri_dev=true dev_url_match=true frontend_1420_ready=true strict_port=true");
