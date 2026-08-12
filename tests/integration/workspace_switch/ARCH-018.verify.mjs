import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(process.env.LOCALBRIDGE_REPO_ROOT ?? ".");
if (process.env.LOCALBRIDGE_ARCH_RULE_ID !== "ARCH-018" || process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR !== "LB-010") process.exit(2);
const controlPath = join(root, "src-tauri", "src", "workspace", "control.rs");
const registryPath = join(root, "src-tauri", "src", "workspace", "registry.rs");
if (!existsSync(controlPath) || !existsSync(registryPath)) process.exit(3);
const control = readFileSync(controlPath, "utf8");
const registry = readFileSync(registryPath, "utf8");
for (const required of [
  "pub fn remove<D: RuntimeDriver>",
  "runtime.stop()",
  "self.data.workspace.clear_active()",
  "self.data.workspace.registry.remove(workspace_id)",
  "WorkspaceRemoval::RemovedRemembered",
  "WorkspaceRemoval::RemovedActive",
]) if (!control.includes(required)) process.exit(4);
for (const required of [
  "Removes LocalBridge metadata only",
  "self.entries.remove(index)",
]) if (!registry.includes(required)) process.exit(5);
const removeStart = control.indexOf("pub fn remove<D: RuntimeDriver>");
const removeEnd = control.indexOf("fn validate_entry", removeStart);
if (removeStart < 0 || removeEnd <= removeStart) process.exit(6);
const removeBody = control.slice(removeStart, removeEnd);
const stop = removeBody.indexOf("runtime.stop()");
const clear = removeBody.indexOf("self.data.workspace.clear_active()");
const metadataRemove = removeBody.indexOf("self.data.workspace.registry.remove(workspace_id)");
if (!(stop >= 0 && clear > stop && metadataRemove > clear)) throw new Error("ARCH-018 active remove order is not runtime.stop -> clear_active -> metadata remove");
const production = `${control}\n${registry}`;
for (const forbidden of [
  "remove_file(", "remove_dir(", "remove_dir_all(", "DeleteFileW", "RemoveDirectoryW", "SHFileOperation", "std::fs::rename(", "fs::rename(", ".trash("
]) if (production.includes(forbidden)) throw new Error(`ARCH-018 filesystem mutation forbidden in workspace removal production: ${forbidden}`);
if (/remove\s*\([^)]*\)[\s\S]{0,500}set_active_reference\s*\(/.test(removeBody)) throw new Error("ARCH-018 remove path auto-selects another remembered workspace");
console.log("ARCH-018_VERIFY=PASS metadata_only_remove=true active_stop_first=true no_filesystem_delete=true no_auto_fallback=true");
