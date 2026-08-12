import { readFileSync } from "node:fs";

const app = readFileSync("src/App.tsx", "utf8");
const onboarding = readFileSync("src/features/onboarding/Onboarding.tsx", "utf8");
const api = readFileSync("src/features/onboarding/api.ts", "utf8");
const frame = readFileSync("src/components/WizardFrame.tsx", "utf8");
const backend = readFileSync("src-tauri/src/commands/onboarding.rs", "utf8");
const lib = readFileSync("src-tauri/src/lib.rs", "utf8");
const auth = JSON.parse(readFileSync("scripts/authorization-records/LB-016.json", "utf8"));
const contracts = JSON.parse(readFileSync("PR_CONTRACTS.json", "utf8"));

const lb016Contract = contracts.prs?.["LB-016"];
if (!lb016Contract) throw new Error("LB-016 machine contract missing");
if (lb016Contract.required_artifacts.includes("6-screen wizard")) throw new Error("LB016_CONTRACT_SEMANTIC_CONFLICT: stale six-screen artifact remains");
if (!lb016Contract.required_artifacts.includes("five-screen onboarding flow") || contracts.rules?.onboarding_screen_count !== 5 || contracts.rules?.onboarding_screen_6_forbidden !== true) {
  throw new Error("LB016_CONTRACT_SEMANTIC_CONFLICT: five-screen contract is not internally consistent");
}

for (const required of [
  'step={1} title="简单设置 即可开始"',
  'LocalBridge是链接ChatGPT与本地代码的工具',
  'step={2} title="连接 OpenAI"',
  'step={3} title="项目与权限"',
  'step={4} title="连接 ChatGPT"',
  'step={5} title="正在准备"',
]) if (!onboarding.includes(required)) throw new Error(`LB-016 screen contract missing: ${required}`);
if (!frame.includes("{step} / 5")) throw new Error("LB-016 frame is not frozen to five screens");
if (/\b6\s*\/\s*6\b|step\s*===\s*6|setStep\(6\)|step=\{6\}/.test(onboarding)) throw new Error("LB-016 contains forbidden sixth screen");

if (!onboarding.includes("Tunnel ID") || !onboarding.includes("运行密钥")) throw new Error("LB-016 OpenAI screen fields missing");
if (!onboarding.includes('type="password"') || !onboarding.includes('autoComplete="off"')) throw new Error("LB-016 runtime key input is not password/no-autocomplete");
if (!onboarding.includes("浏览器存储") || !onboarding.includes("Windows 安全凭据")) throw new Error("LB-016 runtime key secure-storage hint incomplete");
for (const forbidden of ["localStorage", "sessionStorage", "indexedDB"]) if (`${onboarding}\n${api}`.includes(forbidden)) throw new Error(`LB-016 browser storage forbidden: ${forbidden}`);
if (!onboarding.includes('setRuntimeKey("")')) throw new Error("LB-016 runtime key is not cleared after save");

for (const mode of ["edit", "full", "admin"]) if (!onboarding.includes(`\"${mode}\"`)) throw new Error(`LB-016 permission missing: ${mode}`);
if (!onboarding.includes("项目与权限") || !onboarding.includes("管理员模式不会自动弹出系统授权窗口")) throw new Error("LB-016 workspace/permission composition or no-auto-UAC note missing");
if (onboarding.includes("enableAdmin") || onboarding.includes("enable_admin")) throw new Error("LB-016 onboarding must not trigger UAC");
if (/ttl|expires|到期|时长/i.test(onboarding)) throw new Error("LB-016 admin permission added forbidden TTL semantics");

if (!backend.includes('pub const CHATGPT_MCP_SETTINGS_URL: &str = "https://chatgpt.com/"')) throw new Error("LB-016 ChatGPT URL is not a Rust allowlisted https constant");
if (!backend.includes("ShellExecuteW") || !backend.includes("open_allowlisted_url(CHATGPT_MCP_SETTINGS_URL)")) throw new Error("LB-016 ChatGPT link is not opened by system browser adapter");
if (/WebviewWindowBuilder|WebviewUrl|window\.open/.test(`${backend}\n${onboarding}`)) throw new Error("LB-016 ChatGPT flow uses forbidden embedded/arbitrary browser surface");
if (/open_chatgpt_mcp_page\([^)]*url/i.test(backend) || /openChatGpt:\s*\([^)]*(?:url|href|target)[^)]*\)/i.test(api)) throw new Error("LB-016 frontend can supply arbitrary ChatGPT URL");

for (const label of ["本地运行环境", "编码服务", "OpenAI Tunnel"]) if (!onboarding.includes(`label=\"${label}\"`)) throw new Error(`LB-016 readiness check missing: ${label}`);
const readinessCalls = [...onboarding.matchAll(/<ReadinessCheck\s+label=/g)].length;
if (readinessCalls !== 3) throw new Error(`LB-016 screen 5 must have exactly three checks, got ${readinessCalls}`);
if (!onboarding.includes('disabled={!allGreen}') || !onboarding.includes('onClick={() => void finish()}')) throw new Error("LB-016 确定 is not gated by allGreen");
if (!onboarding.includes('allGreen ? <p className="onboarding-success">设置完成，尝试在插件中选择刚刚添加的工具吧！</p> : null')) throw new Error("LB-016 exact success message is not conditional on all green");
if (/useEffect\([\s\S]{0,700}complete\(\)/.test(onboarding)) throw new Error("LB-016 completes onboarding automatically");
if (!backend.includes("if !current.readiness.all_ready()") || !backend.includes("data.settings.onboarding_complete = true")) throw new Error("LB-016 backend completion does not re-check readiness before persistence");

if (!app.includes("if (!onboarding.complete) return <Onboarding")) throw new Error("LB-016 first-run flow is not production-composed ahead of dashboard");
for (const command of ["get_onboarding_state", "save_onboarding_connection", "open_chatgpt_mcp_page", "complete_onboarding"]) if (!lib.includes(`commands::onboarding::${command}`)) throw new Error(`LB-016 command not registered: ${command}`);

const record = auth.records.find((candidate) => candidate.authorization_id === "EXEC-PREAUTH-LB016-001");
const expectedScope = ["src/App.tsx", "src-tauri/src/lib.rs", "scripts/authorization-records/LB-016.json"];
if (!record || record.user_audit_status !== "PENDING" || record.does_not_expand_future_pr_writable_paths !== true || JSON.stringify(record.scope) !== JSON.stringify(expectedScope)) throw new Error("LB-016 preauthorization invalid");

for (const id of ["EXEC-PREAUTH-LB016-002", "EXEC-PREAUTH-LB016-003"]) {
  const extra = auth.records.find((candidate) => candidate.authorization_id === id);
  if (!extra || extra.user_audit_status !== "PENDING" || extra.does_not_expand_future_pr_writable_paths !== true) throw new Error(`LB-016 rework preauthorization invalid: ${id}`);
}

console.log("LB016_CONTRACT=PASS five_screens=true no_screen6=true semantic_conflict=false key_secure=true project_permission_combined=true no_auto_uac=true system_browser_allowlist=true checks=3 confirm_gated=true exact_success=true preauth_pending=3");
