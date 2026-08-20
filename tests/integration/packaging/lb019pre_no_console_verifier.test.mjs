import { readFileSync } from "node:fs";
import { primaryManagedSpawnUsesNoWindow } from "../../../scripts/lb018-release.mjs";

const source = readFileSync("src-tauri/src/runtime/windows_supervisor.rs", "utf8");
const release = readFileSync("scripts/lb018-release.mjs", "utf8");
const behavioral = readFileSync("tests/integration/packaging/lb019pre_release_no_console.rs", "utf8");
if (!primaryManagedSpawnUsesNoWindow(source)) {
  throw new Error("primary managed spawn must carry CREATE_NO_WINDOW");
}

const start = source.indexOf("pub fn spawn(spec: &ManagedProcessSpec)");
const end = source.indexOf("pub const fn snapshot", start);
const primary = source.slice(start, end < 0 ? source.length : end);
const weakenedPrimary = primary.replace(/\s*\|\s*CREATE_NO_WINDOW/, "");
if (weakenedPrimary === primary) throw new Error("negative fixture did not remove primary CREATE_NO_WINDOW");
const weakened = source.slice(0, start) + weakenedPrimary + source.slice(end < 0 ? source.length : end);
if (!weakened.includes("CREATE_NO_WINDOW")) {
  throw new Error("negative fixture must retain helper CREATE_NO_WINDOW tokens");
}
if (primaryManagedSpawnUsesNoWindow(weakened)) {
  throw new Error("verifier false-positive: helper token masked missing primary spawn flag");
}

for (const scenario of [
  "configured_foreground_runtime_start",
  "background_launch",
  "runtime_restart_or_recovery",
  "tunnel_reconnect",
  "login_autostart",
  "managed_shell_or_direct_command_child",
]) {
  if (!release.includes(`\"${scenario}\"`)) throw new Error(`release verifier omits required no-console scenario ${scenario}`);
  if (!behavioral.includes(`NO_CONSOLE_SCENARIO ${scenario}=PASS`)) throw new Error(`behavior test omits required no-console scenario ${scenario}`);
}
if (!release.includes('"--test", "lb019pre_release_no_console"')) throw new Error("release verifier does not execute six-scenario behavior test");
if (!release.includes('buildReleaseTransaction("REFRESH")')) throw new Error("refresh must rebuild instead of rebinding old evidence");
if (!release.includes('artifact installer does not match current NSIS build output')) throw new Error("verify must bind evidence installer to current bundle output");

console.log("LB019PRE_NO_CONSOLE_VERIFIER=PASS primary=true six_scenarios=true refresh_rebuild=true helper_false_positive=false");
