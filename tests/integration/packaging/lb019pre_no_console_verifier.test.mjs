import { readFileSync } from "node:fs";
import { primaryManagedSpawnUsesNoWindow } from "../../../scripts/lb018-release.mjs";

const source = readFileSync("src-tauri/src/runtime/windows_supervisor.rs", "utf8");
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

console.log("LB019PRE_NO_CONSOLE_VERIFIER=PASS primary=true helper_false_positive=false");
