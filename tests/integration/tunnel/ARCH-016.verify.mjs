import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(process.env.LOCALBRIDGE_REPO_ROOT ?? ".");
if (process.env.LOCALBRIDGE_ARCH_RULE_ID !== "ARCH-016") process.exit(2);
if (process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR !== "LB-008") process.exit(3);

const runtimePath = resolve(root, "src-tauri/src/tunnel/runtime.rs");
const configPath = resolve(root, "src-tauri/src/tunnel/config.rs");
if (!existsSync(runtimePath) || !existsSync(configPath)) process.exit(4);
const runtime = readFileSync(runtimePath, "utf8");
const config = readFileSync(configPath, "utf8");

for (const required of [
  'const API_KEY_ENV: &str = "LOCALBRIDGE_RUNTIME_API_KEY"',
  'const API_KEY_REFERENCE: &str = "env:LOCALBRIDGE_RUNTIME_API_KEY"',
  'const TUNNEL_ID_ENV: &str = "CONTROL_PLANE_TUNNEL_ID"',
  '"--control-plane.api-key"',
  "API_KEY_REFERENCE.into()",
  "spec.env(API_KEY_ENV, self.secret.expose_secret())",
  "spec.env(TUNNEL_ID_ENV, self.config.tunnel_id.expose())",
  "spec.env_remove(key)",
  '"CLOUDFLARED_TUNNEL_TOKEN"',
  '"HEALTH_UNIX_SOCKET"',
  '"MCP_SERVER_URL"',
  '"MCP_HTTP_PROXY"',
  '"HARPOON_ADDITIONAL_TRANSPORTS"',
  '"HARPOON_ALLOW_PLAINTEXT_HTTP"',
  '"CONTROL_PLANE_POLL_CHANNELS"',
  '"TUNNEL_CLIENT_CONFIG"',
  '"CONTROL_PLANE_API_KEY"',
  '"OPENAI_API_KEY"',
  "SecretInjectionUnsupported",
  "actual_process_command_line_never_contains_runtime_secret_and_job_stop_drains",
  "Get-CimInstance Win32_Process",
  "assert!(!command_line.contains(SECRET_ONE))",
  "assert!(!command_line.contains(TUNNEL_ID))",
  "assert!(command_line.contains(API_KEY_REFERENCE))",
]) {
  if (!runtime.includes(required)) throw new Error(`ARCH-016 missing enforcement evidence: ${required}`);
}

if (!config.includes('const OPENAI_CONTROL_PLANE: &str = "https://api.openai.com"')) {
  throw new Error("ARCH-016 production control-plane pin missing");
}
if (!runtime.includes('"--health.listen-addr"') || !runtime.includes('"127.0.0.1:0"')) {
  throw new Error("ARCH-016 loopback health contract missing");
}

const argsStart = runtime.indexOf("pub fn command_line_arguments");
const argsEnd = runtime.indexOf("pub fn health_url_file", argsStart);
if (argsStart < 0 || argsEnd <= argsStart) throw new Error("ARCH-016 could not isolate argv builder");
const argvBuilder = runtime.slice(argsStart, argsEnd);
if (/expose_secret\s*\(/.test(argvBuilder)) {
  throw new Error("ARCH-016 secret exposure appears in command-line builder");
}
for (const forbidden of ["--cloudflared.managed", "--cloudflared.path", "--cloudflared.token"]) {
  if (argvBuilder.includes(forbidden)) {
    throw new Error(`ARCH-016 ordinary Tunnel startup must not force optional managed Cloudflare flag: ${forbidden}`);
  }
}
if (/LOCALBRIDGE_RUNTIME_API_KEY_ONE|LB008_SYNTHETIC_RUNTIME_KEY/.test(argvBuilder)) {
  throw new Error("ARCH-016 synthetic/raw secret literal appears in command-line builder");
}

const exposeUses = [...runtime.matchAll(/\.expose_secret\s*\(\s*\)/g)].length;
if (exposeUses !== 1) throw new Error(`ARCH-016 unexpected secret exposure call count: ${exposeUses}`);
const envInjection = /spec\s*=\s*spec\.env\(API_KEY_ENV,\s*self\.secret\.expose_secret\(\)\)/s;
if (!envInjection.test(runtime)) throw new Error("ARCH-016 secret exposure is not confined to child env injection");

console.log("ARCH-016_VERIFY=PASS env_only_secret=true os_command_line_regression=true tunnel_id_env_only=true inherited_override_removal=true ordinary_tunnel_no_managed_cloudflare=true");
