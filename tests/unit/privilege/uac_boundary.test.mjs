import { readFileSync } from "node:fs";

const windows = readFileSync("src-tauri/src/privilege/windows.rs", "utf8");
const main = readFileSync("src-tauri/src/main.rs", "utf8");
const lib = readFileSync("src-tauri/src/lib.rs", "utf8");
const tauri = readFileSync("src-tauri/tauri.conf.json", "utf8");
const broker = readFileSync("src-tauri/bin/privileged-broker/main.rs", "utf8");

for (const required of ["ShellExecuteExW", 'OsStr::new("runas")', "SEE_MASK_NOCLOSEPROCESS", "launch_broker_with_explicit_uac"]) {
  if (!windows.includes(required)) throw new Error(`LB-011 explicit UAC path missing: ${required}`);
}
for (const forbidden of ["nonce", "secret", "token", "password"]) {
  const parameterBuilder = windows.slice(windows.indexOf("fn build_uac_parameters"), windows.indexOf("fn current_user_sid_string"));
  if (parameterBuilder.toLowerCase().includes(`--${forbidden}`)) throw new Error(`LB-011 UAC CLI contains secret-like parameter: ${forbidden}`);
}
for (const source of [main, lib]) {
  if (source.includes("launch_broker_with_explicit_uac") || source.includes('"runas"')) throw new Error("LB-011 startup path auto-invokes UAC");
}
if (/requireAdministrator|highestAvailable/i.test(tauri)) throw new Error("LocalBridge whole-app manifest requests elevation");
if (/exec|program|command|shell/i.test(broker)) throw new Error("LB-011 broker foundation binary exposes execution behavior");
console.log("LB011_UAC_BOUNDARY=PASS explicit_only=true broker_only=true no_secret_cli=true whole_app_unelevated=true");
