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

forbidTree(
  "src-tauri/src/commands",
  /(?:pub\s+async\s+fn|#\[tauri::command\])[\s\S]{0,240}->\s*Result<[^,>]+,\s*String>/,
  "a Tauri UI command exposes Result<T, String>",
);
if (failures.length > 0) {
  for (const failure of failures) process.stderr.write(`schema44: ${failure}\n`);
  process.exit(1);
}

process.stdout.write("schema44 architecture residue scan passed; behavioral invariants run in the Rust test stage\n");
