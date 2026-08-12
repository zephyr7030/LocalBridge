import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(process.env.LOCALBRIDGE_REPO_ROOT ?? ".");
if (process.env.LOCALBRIDGE_ARCH_RULE_ID !== "ARCH-009") throw new Error("ARCH-009 verifier invoked for wrong rule");
if (process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR !== "LB-006") throw new Error("ARCH-009 activation PR mismatch");

const read = (path) => readFileSync(join(root, path), "utf8");
for (const path of [
  "src-tauri/src/mcp/mod.rs",
  "src-tauri/src/mcp/bundle.rs",
  "src-tauri/src/mcp/http.rs",
  "src-tauri/src/mcp/runtime.rs",
  "runtime-manifest.toml",
]) if (!existsSync(join(root, path))) throw new Error(`stable adapter artifact missing: ${path}`);

const mod = read("src-tauri/src/mcp/mod.rs");
for (const module of ["bundle", "http", "runtime"]) if (!new RegExp(`mod\\s+${module}\\s*;`).test(mod)) throw new Error(`stable adapter module missing: ${module}`);
for (const exported of ["CodingToolsRuntime", "CodingToolsRuntimeConfig", "CodingToolsRuntimeError", "InternalBearer"]) if (!mod.includes(exported)) throw new Error(`stable adapter export missing: ${exported}`);

const runtime = read("src-tauri/src/mcp/runtime.rs");
const bundle = read("src-tauri/src/mcp/bundle.rs");
const http = read("src-tauri/src/mcp/http.rs");
const production = `${runtime}\n${bundle}\n${http}`;
for (const required of [
  "WindowsProcessSupervisor",
  "ManagedProcessSpec",
  'join("runtime").join("python")',
  'join("python.exe")',
  '"127.0.0.1"',
  "CODING_TOOLS_MCP_AUTH_TOKEN",
  "CODING_TOOLS_MCP_TELEMETRY",
  '"off"',
  "DO_NOT_TRACK",
  "RuntimeChecksumMismatch",
  "RuntimeMissing",
  "Mcp-Session-Id",
]) if (!production.includes(required)) throw new Error(`stable adapter invariant missing: ${required}`);

for (const forbidden of [
  /std::process::Command|Command::new\s*\(/,
  /(?:^|["'`\s])py(?:\.exe)?(?:["'`\s]|$)/mi,
  /python(?:\.exe)?\s+from\s+PATH/i,
  /user\s+site-packages/i,
  /pip\s+install/i,
  /Invoke-WebRequest/i,
  /--auth-token/,
  /\.arg\s*\([^\n]{0,100}CODING_TOOLS_MCP_AUTH_TOKEN/,
  /CodingToolsPermissionMode\s*::\s*Dangerous/,
  /\.arg\s*\(\s*"dangerous"\s*\)/,
  /(?:TcpListener|TcpStream)::(?:bind|connect)[^\n]*(?:0\.0\.0\.0|\[::\]|"::")/,
]) if (forbidden.test(production)) throw new Error(`stable adapter forbidden coupling/pattern: ${forbidden}`);

const stateFiles = ["src-tauri/src/state/runtime.rs", "src-tauri/src/state/mod.rs"]
  .map(read)
  .join("\n");
for (const privateCoupling of ["coding_tools_mcp", "server.py", "McpSession", "PYTHON_VERSION"]) if (stateFiles.includes(privateCoupling)) throw new Error(`domain depends on external runtime private structure: ${privateCoupling}`);

const manifest = read("runtime-manifest.toml");
for (const required of [
  'stable_adapter = "src-tauri/src/mcp"',
  'executable = "runtime/python/python.exe"',
  'bind_host = "127.0.0.1"',
  'auth_secret_injection = "child_environment:CODING_TOOLS_MCP_AUTH_TOKEN"',
  'runtime_pip_install = false',
  'external_python_fallback = false',
]) if (!manifest.includes(required)) throw new Error(`runtime manifest stable adapter invariant missing: ${required}`);

console.log("ARCH-009_VERIFY=PASS stable_adapter=true bundled_python_only=true loopback=true bearer_child_env=true no_runtime_pip=true");
