import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const launcher = readFileSync("start-localbridge.cmd", "utf8");
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

console.log("LOCALBRIDGE_LAUNCHER_TEST=PASS offline_check=true no_powershell=true no_install=true exact_tauri_dev=true");
