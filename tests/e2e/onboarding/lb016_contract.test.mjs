import { readFileSync } from "node:fs";

const read = (path) => readFileSync(path, "utf8");
const app = read("src/App.tsx");
const onboarding = read("src/features/onboarding/Onboarding.tsx");
const api = read("src/features/onboarding/api.ts");
const onboardingCss = read("src/features/onboarding/onboarding.css");
const sharedCss = read("src/styles.css");
const frame = read("src/components/WizardFrame.tsx");
const backend = read("src-tauri/src/commands/onboarding.rs");
const uiBackend = read("src-tauri/src/commands/ui.rs");
const tunnelHealth = read("src-tauri/src/tunnel/health.rs");
const tunnelRuntime = read("src-tauri/src/tunnel/runtime.rs");
const tunnelMod = read("src-tauri/src/tunnel/mod.rs");
const orchestrator = read("src-tauri/src/runtime/orchestrator.rs");
const background = read("src-tauri/src/app/background.rs");
const tray = read("src-tauri/src/tray/mod.rs");
const main = read("src-tauri/src/main.rs");
const resizeE2e = read("tests/e2e/onboarding/resize_runtime_e2e.mjs");
const lib = read("src-tauri/src/lib.rs");
const auth = JSON.parse(read("scripts/authorization-records/LB-016.json"));
const contracts = JSON.parse(read("PR_CONTRACTS.json"));

const lb016 = contracts.prs?.["LB-016"];
if (!lb016) throw new Error("LB-016 machine contract missing");
const exactSuccessCopy = "配置完成，在插件中选择刚刚添加的Local Bridge试试吧";
if (!lb016.required_artifacts.includes("six-screen onboarding flow")
  || contracts.rules?.onboarding_screen_count !== 6
  || contracts.rules?.onboarding_screen_7_forbidden !== true) {
  throw new Error("LB016_CONTRACT_SEMANTIC_CONFLICT: six-screen contract is not internally consistent");
}
if (lb016.required_artifacts.some((item) => /five-screen|5-screen/i.test(item))) throw new Error("LB-016 retains stale five-screen artifact");
if (contracts.rules?.onboarding_user_facing_connector_term !== "Local Bridge"
  || contracts.rules?.onboarding_screen_6_success_message !== exactSuccessCopy
  || JSON.stringify(contracts.rules?.onboarding_window_default_inner_size) !== JSON.stringify([900, 620])
  || JSON.stringify(contracts.rules?.onboarding_window_min_inner_size) !== JSON.stringify([720, 500])
  || contracts.rules?.onboarding_window_resizable !== true
  || contracts.rules?.onboarding_viewport_responsive_required !== true
  || contracts.rules?.onboarding_fixed_card_min_height_forbidden !== true
  || contracts.rules?.main_webview_matches_native_client_area_on_resize_required !== true
  || contracts.rules?.dashboard_viewport_responsive_required !== true
  || contracts.rules?.real_windows_tauri_resize_e2e_required !== true
  || contracts.rules?.static_resize_markers_alone_are_insufficient !== true) {
  throw new Error("LB-016 machine contract does not freeze Local Bridge success/responsive semantics");
}
for (const artifact of [
  "viewport-responsive resizable onboarding layout",
  "native-window/WebView client-area resize synchronization",
]) if (!lb016.required_artifacts.includes(artifact)) throw new Error(`LB-016 responsive runtime artifact is not frozen: ${artifact}`);
for (const required of [
  `screen 6 success message is ${exactSuccessCopy}`,
  "screens 4 and 5 use Local Bridge as the user-facing connector term",
  "resizable onboarding window has a 720x500 minimum and wizard body adapts to viewport height without fixed card minimum height",
  "real Windows Tauri resize E2E cross-checks native client area against live WebView JS viewport for two native sizes plus maximize and proves Dashboard and onboarding reflow",
]) if (!lb016.required_tests.includes(required)) throw new Error(`LB-016 authorized rework test contract missing: ${required}`);

for (const required of [
  'step={1} title="简单设置 即可开始"',
  "LocalBridge是链接ChatGPT与本地代码的工具",
  'step={2} title="OpenAI 设置"',
  'step={3} title="项目与权限"',
  'step={4} title="Local Bridge 设置"',
  'step={5} title="Local Bridge 使用确认"',
  'step={6} title="启动检查"',
]) if (!onboarding.includes(required)) throw new Error(`LB-016 screen contract missing: ${required}`);
if (!frame.includes("{step} / 6")) throw new Error("LB-016 frame is not frozen to six screens");
if (/\b7\s*\/\s*7\b|step\s*===\s*7|setStep\(7\)|step=\{7\}/.test(`${frame}\n${onboarding}`)) throw new Error("LB-016 contains forbidden seventh screen");

for (const label of [">Tunnel ID</label>", ">Runtime API Key</label>"]) if (!onboarding.includes(label)) throw new Error(`LB-016 OpenAI field missing: ${label}`);
if (!onboarding.includes('type="password"') || !onboarding.includes('autoComplete="off"')) throw new Error("LB-016 Runtime API Key input is not password/no-autocomplete");
if (!onboarding.includes("Runtime API Key 仅保存在 Windows 安全凭据中") || !onboarding.includes("浏览器存储")) throw new Error("LB-016 Runtime API Key secure-storage hint incomplete");
for (const forbidden of ["localStorage", "sessionStorage", "indexedDB"]) if (`${onboarding}\n${api}`.includes(forbidden)) throw new Error(`LB-016 browser storage forbidden: ${forbidden}`);
if (!onboarding.includes('setRuntimeKey("")')) throw new Error("LB-016 Runtime API Key is not cleared after save");
for (const marker of ["openTunnelSettings", "openApiKeys", 'invoke<void>("open_openai_tunnel_settings")', 'invoke<void>("open_openai_api_keys")']) if (!`${onboarding}\n${api}`.includes(marker)) throw new Error(`LB-016 Screen 2 setup entry missing: ${marker}`);

for (const mode of ['"edit"', '"full"', '"admin"']) if (!onboarding.includes(mode)) throw new Error(`LB-016 permission missing: ${mode}`);
if (!onboarding.includes("管理员模式不会自动弹出系统授权窗口")) throw new Error("LB-016 no-auto-UAC note missing");
if (/enableAdmin|enable_admin/.test(onboarding) || /ttl|expires|到期|时长/i.test(onboarding)) throw new Error("LB-016 onboarding adds forbidden UAC/TTL semantics");
if (!onboarding.includes("onboardingApi.chooseWorkspaceFolder()") || !api.includes('invoke<string | null>("choose_onboarding_workspace_folder")')) throw new Error("LB-016 native folder picker frontend path missing");
for (const marker of ["SHBrowseForFolderW", "SHGetPathFromIDListW", "BIF_RETURNONLYFSDIRS", "choose_onboarding_workspace_folder"]) if (!backend.includes(marker)) throw new Error(`LB-016 native Windows folder picker backend missing: ${marker}`);
if (/project-path-onboarding|输入代码文件夹路径|手动输入.*路径/.test(onboarding)) throw new Error("LB-016 Screen 3 still exposes manual absolute-path entry as primary interaction");
if (!api.includes('rememberProject: (path: string) => invoke<string>("add_project", { path, deferActivation: true })')) throw new Error("LB-016 deferred project persistence API missing");
const addProjectStart = uiBackend.indexOf("pub fn add_project");
const selectProjectStart = uiBackend.indexOf("pub fn select_project", addProjectStart);
const addProjectFlow = addProjectStart >= 0 && selectProjectStart > addProjectStart ? uiBackend.slice(addProjectStart, selectProjectStart) : "";
const compactAddProjectFlow = addProjectFlow.replace(/\s+/g, "");
for (const marker of ["defer_activation:Option<bool>", "ifdefer_activation.unwrap_or(false)", "store.save(&data)", "returnOk(id_value)"]) if (!compactAddProjectFlow.includes(marker)) throw new Error(`LB-016 deferred project persistence missing: ${marker}`);

const connectorUrl = "https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins";
if (!backend.includes(`CHATGPT_CUSTOM_CONNECTOR_URL: &str = "${connectorUrl}"`)) throw new Error("LB-016 fixed ChatGPT custom connector URL missing");
for (const marker of ["ShellExecuteW", "open_allowlisted_url(CHATGPT_CUSTOM_CONNECTOR_URL)", "open_chatgpt_custom_connector_settings"]) if (!backend.includes(marker)) throw new Error(`LB-016 system-browser connector adapter missing: ${marker}`);
if (!onboarding.includes("点击“打开 Local Bridge 设置”") || !onboarding.includes("新建 Local Bridge") || !onboarding.includes("保存后返回 LocalBridge")) throw new Error("LB-016 Screen 4 foolproof Local Bridge guidance incomplete");
if (onboarding.includes("连接器")) throw new Error("LB-016 user-visible onboarding terminology regressed from Local Bridge to 连接器");
if (!api.includes('openConnectorSettings: () => invoke<void>("open_chatgpt_custom_connector_settings")')) throw new Error("LB-016 frontend connector action is not argument-free");
if (/WebviewWindowBuilder|WebviewUrl|window\.open/.test(`${backend}\n${onboarding}\n${api}`)) throw new Error("LB-016 uses forbidden embedded/arbitrary browser surface");

const compactBackend = backend.replace(/\s+/g, "");
for (const marker of ["pubstructConnectorEndpointProjection", "pubfnget_connector_endpoint", "lifecycle.connector_endpoint()"] ) if (!compactBackend.includes(marker)) throw new Error(`LB-016 typed endpoint UI projection missing: ${marker}`);
for (const marker of ["pub struct ConnectorEndpoint", "mcp_url: Option<String>", "from_verified_metadata", 'starts_with("https://")', "connector_endpoint_comes_only_from_explicit_valid_https_metadata", "mcp_url_path"]) if (!tunnelHealth.includes(marker)) throw new Error(`LB-016 verified Tunnel metadata endpoint contract missing: ${marker}`);
if (!tunnelRuntime.includes("self.connector_endpoint = probe.connector_endpoint") || !tunnelRuntime.includes("pub fn connector_endpoint(&self) -> Option<ConnectorEndpoint>")) throw new Error("LB-016 Tunnel runtime does not retain typed connector endpoint");
if (!tunnelMod.includes("pub use health::ConnectorEndpoint") || !orchestrator.includes("pub fn connector_endpoint(&self) -> Option<ConnectorEndpoint>") || !background.includes("pub fn connector_endpoint(&self) -> Option<ConnectorEndpoint>")) throw new Error("LB-016 typed connector endpoint does not cross the stable Rust lifecycle boundary");
if (!api.includes('readConnectorEndpoint: () => invoke<ConnectorEndpointProjection>("get_connector_endpoint")')) throw new Error("LB-016 frontend does not consume typed Rust endpoint projection");
if (!onboarding.includes("navigator.clipboard.writeText(connectorEndpoint)")) throw new Error("LB-016 copy action does not use projected endpoint");
if (/\/v1\/mcp|mcp_url_path|mcpUrlPath/.test(`${onboarding}\n${api}`)) throw new Error("LB-016 frontend contains connector endpoint derivation material");
if (/tunnelId\s*[+`]|[+`]\s*tunnelId/.test(`${onboarding}\n${api}`)) throw new Error("LB-016 frontend derives a connector endpoint from Tunnel ID");

if (!onboarding.includes("LocalBridge 不判断 ChatGPT 是否已连接")) throw new Error("LB-016 Screen 5 does not explicitly avoid fake ChatGPT state");
for (const marker of ["Local Bridge 使用确认", "Local Bridge 地址将在 OpenAI Tunnel 就绪后显示。", "返回 ChatGPT 后即可尝试选择 Local Bridge"]) if (!onboarding.includes(marker)) throw new Error(`LB-016 Local Bridge user-facing terminology missing: ${marker}`);
for (const forbidden of ["ChatGPT 已连接", "已成功连接 ChatGPT", "连接 ChatGPT 成功"]) if (onboarding.includes(forbidden)) throw new Error(`LB-016 Screen 5 fabricates ChatGPT state: ${forbidden}`);
if (!onboarding.includes('className="onboarding-copy-feedback"') || !/\.onboarding-copy-feedback\{[^}]*min-height\s*:\s*20px/i.test(onboardingCss)) throw new Error("LB-016 copy feedback does not reserve layout space");

if (/\.onboarding-(?:primary|secondary)\b/.test(onboardingCss) || /onboarding-(?:primary|secondary)/.test(onboarding)) throw new Error("LB-016 still has a separate onboarding button system");
for (const className of ['className="primary"', 'className="secondary"', "choice onboarding-permission"]) if (!onboarding.includes(className)) throw new Error(`LB-016 does not use shared button class: ${className}`);
for (const selector of [".primary", ".secondary", ".ghost", ".choice"]) if (!sharedCss.includes(selector)) throw new Error(`LB-016 shared LB-015 button selector missing: ${selector}`);

for (const label of ["本地运行环境", "编码服务", "OpenAI Tunnel"]) if (!onboarding.includes(`label=\"${label}\"`)) throw new Error(`LB-016 Screen 6 readiness check missing: ${label}`);
const readinessCalls = [...onboarding.matchAll(/<ReadinessCheck\s+label=/g)].length;
if (readinessCalls !== 3) throw new Error(`LB-016 Screen 6 must have exactly three checks, got ${readinessCalls}`);
if (!onboarding.includes('disabled={!allGreen}') || !onboarding.includes('onClick={() => void finish()}')) throw new Error("LB-016 Screen 6 confirm is not gated by allGreen");
const success = `allGreen ? <p className="onboarding-success">${exactSuccessCopy}</p> : null`;
if (!onboarding.includes(success)) throw new Error("LB-016 exact Screen 6 success message is not conditional on all green");
if (onboarding.includes("设置完成，尝试在 ChatGPT 中选择刚刚添加的连接器吧！")) throw new Error("LB-016 stale Screen 6 success copy remains in production");

for (const marker of [".inner_size(900.0, 620.0)", ".min_inner_size(720.0, 500.0)", ".resizable(true)"]) if (!tray.includes(marker)) throw new Error(`LB-016 resizable window contract missing: ${marker}`);
for (const marker of ["sync_main_webview_to_client", "PhysicalPosition::new(0, 0)", "webview.set_bounds"]) if (!tray.includes(marker)) throw new Error(`LB-016 native WebView client-area synchronization missing: ${marker}`);
if (!main.includes("WindowEvent::Resized(client_size)") || !main.includes("sync_main_webview_to_client(window.app_handle(), *client_size)")) throw new Error("LB-016 native resize event is not bound to WebView client-area synchronization");
for (const marker of ["tauri.cmd dev --no-watch", "LOCALBRIDGE_RESIZE_E2E_VIEW", "CARGO_TARGET_DIR", "LB016_REAL_RESIZE_E2E=PASS"]) if (!resizeE2e.includes(marker)) throw new Error(`LB-016 true runtime resize E2E runner missing: ${marker}`);
for (const marker of ["window.inner_size()", "window.eval(RESIZE_E2E_METRICS_SCRIPT)", "invoke('resize_e2e_report'", "ResizeE2eMetricsSink::new", "window.innerWidth", "window.innerHeight", "document.documentElement.clientWidth", "document.getElementById('root')", ".onboarding-shell", ".shell", "window.maximize()", "LB016_REAL_RESIZE_E2E=PASS"]) if (!main.includes(marker)) throw new Error(`LB-016 live Tauri/WebView resize assertions missing: ${marker}`);
for (const marker of ["ResizeE2eMetricsSink", "resize_e2e_report", "cfg(debug_assertions)", "cfg(not(debug_assertions))", "localbridge_invoke_handler![]"]) if (!lib.includes(marker)) throw new Error(`LB-016 debug-only resize IPC contract missing: ${marker}`);
if (!/\.onboarding-shell\{[^}]*min-height:100vh[^}]*height:100dvh[^}]*overflow:hidden/is.test(onboardingCss.replace(/\s+/g, ""))) throw new Error("LB-016 onboarding shell is not viewport-height constrained");
if (!/\.onboarding-card\{[^}]*max-height:100%[^}]*min-height:0/is.test(onboardingCss.replace(/\s+/g, ""))) throw new Error("LB-016 onboarding card still relies on a rigid minimum height");
if (!/\.onboarding-body\{[^}]*min-height:0[^}]*overflow-y:auto/is.test(onboardingCss.replace(/\s+/g, ""))) throw new Error("LB-016 wizard body cannot scroll within a resized viewport");
if (/min-height:(?:500|540)px/i.test(onboardingCss.replace(/\s+/g, ""))) throw new Error("LB-016 stale fixed wizard card minimum height remains");
if (!/@media\(max-height:560px\)/i.test(onboardingCss.replace(/\s+/g, ""))) throw new Error("LB-016 lacks viewport-height responsive spacing rules");
const completionCalls = [...onboarding.matchAll(/onboardingApi\.complete\(\)/g)];
const finishStart = onboarding.indexOf("const finish = async () =>");
const screenStart = onboarding.indexOf("if (step === 1)", finishStart);
if (completionCalls.length !== 1 || finishStart < 0 || screenStart <= finishStart
  || completionCalls[0].index <= finishStart || completionCalls[0].index >= screenStart) throw new Error("LB-016 completion is not restricted to the explicit finish handler");
if (!backend.includes("if !current.readiness.all_ready()") || !backend.includes("data.settings.onboarding_complete = true")) throw new Error("LB-016 backend completion does not re-check readiness before persistence");

if (!app.includes("if (!onboarding.complete) return <Onboarding")) throw new Error("LB-016 first-run flow is not production-composed ahead of dashboard");
for (const command of [
  "get_onboarding_state",
  "save_onboarding_connection",
  "open_openai_tunnel_settings",
  "open_openai_api_keys",
  "open_chatgpt_custom_connector_settings",
  "get_connector_endpoint",
  "choose_onboarding_workspace_folder",
  "complete_onboarding",
]) if (!lib.includes(`commands::onboarding::${command}`)) throw new Error(`LB-016 command not registered: ${command}`);

const baseAuth = auth.records.find((candidate) => candidate.authorization_id === "EXEC-PREAUTH-LB016-001");
if (!baseAuth || baseAuth.user_audit_status !== "PENDING" || baseAuth.does_not_expand_future_pr_writable_paths !== true) throw new Error("LB-016 production-composition preauthorization invalid");
const reworkAuth = auth.records.find((candidate) => candidate.authorization_id === "EXEC-PREAUTH-LB016-006");
const expectedReworkScope = [
  "src-tauri/Cargo.toml",
  "src-tauri/src/app/background.rs",
  "src-tauri/src/runtime/orchestrator.rs",
  "src-tauri/src/tunnel/health.rs",
  "src-tauri/src/tunnel/mod.rs",
  "src-tauri/src/tunnel/runtime.rs",
  "scripts/authorization-records/LB-016.json",
];
if (!reworkAuth || JSON.stringify(reworkAuth.scope) !== JSON.stringify(expectedReworkScope)
  || reworkAuth.user_audit_status !== "PENDING"
  || reworkAuth.does_not_expand_future_pr_writable_paths !== true
  || !reworkAuth.evidence_ref.includes("2026-08-13")) throw new Error("LB-016 six-screen rework preauthorization invalid");

const semanticAuth = auth.records.find((candidate) => candidate.authorization_id === "EXEC-PREAUTH-LB016-007");
const expectedSemanticScope = [
  "AGENTS.md",
  "START_HERE.md",
  "PR_CONTRACTS.json",
  "PROJECT_STATE.json",
  "docs/01_PRODUCT_UX.md",
  "docs/04_UX_SPEC.md",
  "docs/06_PR_PLAN.md",
  "docs/07_ACCEPTANCE.md",
  "docs/07_ACCEPTANCE_MATRIX.md",
  "docs/08_FINAL_REVIEW.md",
  "skills/ui/SKILL.md",
  "src-tauri/src/tray/mod.rs",
  "scripts/verify-architecture/g4-human-gate.mjs",
  "scripts/verify-architecture/g4-human-gate.test.mjs",
  "scripts/authorization-records/LB-016.json",
];
if (!semanticAuth || JSON.stringify(semanticAuth.scope) !== JSON.stringify(expectedSemanticScope)
  || semanticAuth.user_audit_status !== "PENDING"
  || semanticAuth.does_not_expand_future_pr_writable_paths !== true
  || !semanticAuth.evidence_ref.includes("2026-08-13")
  || !semanticAuth.evidence_ref.includes(exactSuccessCopy)) throw new Error("LB-016 Local Bridge/responsive preauthorization invalid");

const resizeAuth = auth.records.find((candidate) => candidate.authorization_id === "EXEC-PREAUTH-LB016-009");
const expectedResizeScope = [
  "src-tauri/src/main.rs",
  "src-tauri/src/lib.rs",
  "src-tauri/src/tray/mod.rs",
  "PR_CONTRACTS.json",
  "AGENTS.md",
  "docs/01_PRODUCT_UX.md",
  "docs/04_UX_SPEC.md",
  "docs/06_PR_PLAN.md",
  "docs/07_ACCEPTANCE.md",
  "docs/07_ACCEPTANCE_MATRIX.md",
  "docs/08_FINAL_REVIEW.md",
  "scripts/verify-architecture/g4-human-gate.mjs",
  "scripts/verify-architecture/g4-human-gate.test.mjs",
  "scripts/authorization-records/LB-016.json",
  "START_HERE.md",
  "PROJECT_STATE.json",
  "skills/ui/SKILL.md",
];
if (!resizeAuth || JSON.stringify(resizeAuth.scope) !== JSON.stringify(expectedResizeScope)
  || resizeAuth.user_audit_status !== "PENDING"
  || resizeAuth.does_not_expand_future_pr_writable_paths !== true
  || !resizeAuth.evidence_ref.includes("2026-08-13")
  || !resizeAuth.evidence_ref.includes("true resize E2E")) throw new Error("LB-016 native/WebView resize preauthorization invalid");

console.log("LB016_CONTRACT=PASS six_screens=true no_screen7=true key_secure=true native_folder_picker=true fixed_connector_deeplink=true typed_verified_endpoint=true frontend_endpoint_derivation=false fake_chatgpt_state=false copy_feedback_reserved=true shared_buttons=true checks=3 confirm_gated=true exact_success=true local_bridge_term=true responsive_window=true native_webview_sync=true real_resize_e2e_required=true static_markers_insufficient=true rework_preauth=EXEC-PREAUTH-LB016-006 semantic_preauth=EXEC-PREAUTH-LB016-007 resize_preauth=EXEC-PREAUTH-LB016-009");
