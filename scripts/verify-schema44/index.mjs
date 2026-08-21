import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(scriptDir, "..", "..");
const failures = [];

function read(relative) {
  return fs.readFileSync(path.join(root, relative), "utf8");
}

function walk(relative) {
  const absolute = path.join(root, relative);
  return fs.readdirSync(absolute, { withFileTypes: true }).flatMap((entry) => {
    const child = path.join(relative, entry.name);
    return entry.isDirectory() ? walk(child) : [child];
  });
}

function requireText(relative, pattern, message) {
  if (!pattern.test(read(relative))) failures.push(message);
}

function forbidText(relative, pattern, message) {
  if (pattern.test(read(relative))) failures.push(message);
}

function forbidTree(relative, pattern, message) {
  for (const file of walk(relative).filter((value) => value.endsWith(".rs"))) {
    if (pattern.test(read(file))) failures.push(`${message}: ${file}`);
  }
}

forbidTree(
  "src-tauri/src/domain",
  /\bserde_json::Value\b|\buse\s+serde_json::\{[^}]*\bValue\b/,
  "core domain contains opaque serde_json::Value state",
);
forbidTree(
  "src-tauri/src/control_plane",
  /\bserde_json::Value\b|\buse\s+serde_json::\{[^}]*\bValue\b/,
  "control plane contains opaque serde_json::Value state",
);

for (const relative of [
  "src-tauri/src/runtime",
  "src-tauri/src/privilege",
  "src-tauri/src/workspace",
  "src-tauri/src/control_plane",
  "src-tauri/src/domain",
  "src-tauri/src/execution",
  "src-tauri/src/filesystem",
]) {
  forbidTree(relative, /\bcrate::mcp\b|\bsuper::mcp\b/, "domain-side module depends on MCP transport");
}

for (const relative of [
  "src-tauri/src/mcp/filesystem_service.rs",
  "src-tauri/src/mcp/path_authority.rs",
  "src-tauri/src/mcp/shell.rs",
  "src-tauri/src/mcp/task_state.rs",
  "src-tauri/src/mcp/workflow_checkpoint.rs",
  "src-tauri/src/mcp/verification_planner.rs",
  "src-tauri/src/mcp/context_service.rs",
  "src-tauri/src/mcp/edit_service.rs",
]) {
  if (fs.existsSync(path.join(root, relative))) failures.push(`retired MCP domain file remains: ${relative}`);
}

forbidTree("src-tauri/src", /terminal_payload/, "parallel public-session terminal truth remains");
forbidTree("src-tauri/src", /\bcancel_all\b/, "unscoped cancel-all operation remains");
forbidTree("src-tauri/src", /HashMap\s*<\s*RpcRequestId/, "global bare request-id registry remains");
forbidTree(
  "src-tauri/src",
  /(?:RwLock|Mutex)\s*<\s*PermissionMode\s*>/,
  "parallel mutable permission truth remains",
);

requireText(
  "src-tauri/src/mcp/facade.rs",
  /"task_id":\{"type":"string","minLength":1/,
  "task_control does not publish a task_id selector",
);
requireText("src-tauri/src/mcp/facade.rs", /TaskIdRequired/, "TaskIdRequired is missing");
requireText("src-tauri/src/mcp/facade.rs", /TaskNotOwned/, "TaskNotOwned is missing");
requireText(
  "src-tauri/src/mcp/server.rs",
  /cancel_queued_task\(&session_id, task_id\)/,
  "task cancellation is not task scoped",
);
const server = read("src-tauri/src/mcp/server.rs");
if ((server.match(/cancel_queued_by_session/g) ?? []).length !== 1) {
  failures.push("session-wide queue cancellation escaped the explicit session-close path");
}

requireText(
  "src-tauri/src/commands/error.rs",
  /type\s+UiResult<T>\s*=\s*Result<T,\s*UiError>/,
  "UI commands do not share the typed UiError boundary",
);
forbidTree(
  "src-tauri/src/commands",
  /(?:pub\s+async\s+fn|#\[tauri::command\])[\s\S]{0,240}->\s*Result<[^,>]+,\s*String>/,
  "a Tauri UI command exposes Result<T, String>",
);
requireText(
  "src-tauri/src/commands/ui.rs",
  /let\s+control_plane\s*=\s*lifecycle\.control_plane_snapshot\(\);/,
  "main UI projection is not derived from one control-plane snapshot",
);
requireText(
  "src-tauri/src/control_plane/resource_lifecycle.rs",
  /RESOURCE_LIFECYCLE_POLICIES:\s*\[ResourceLifecyclePolicy;\s*9\]/,
  "the required resource lifecycle catalog is incomplete",
);
requireText(
  "src-tauri/src/control_plane/execution_registry.rs",
  /PreserveTerminalAndMarkUnfinishedLost|lost_terminal\(\)/,
  "execution restart recovery does not converge unfinished work to Lost",
);
requireText(
  "src-tauri/src/mcp/server.rs",
  /command_control_kill_is_not_blocked_by_unrelated_foreground_work/,
  "command control has no executable proof that Work cannot block Control",
);

if (failures.length > 0) {
  for (const failure of failures) process.stderr.write(`schema44: ${failure}\n`);
  process.exit(1);
}

process.stdout.write("schema44 architecture residue verification passed\n");
