import { existsSync, readFileSync } from "node:fs";

const read = (path) => readFileSync(path, "utf8");
const app = read("src/App.tsx");
const chrome = read("src/components/WindowChrome.tsx");
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
const lib = read("src-tauri/src/lib.rs");
const windowCapability = JSON.parse(read("src-tauri/capabilities/window-chrome.json"));
const fixedWindowE2e = read("tests/e2e/onboarding/fixed_window_runtime_e2e.mjs");
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
  || JSON.stringify(contracts.rules?.onboarding_window_min_inner_size) !== JSON.stringify([900, 620])
  || JSON.stringify(contracts.rules?.onboarding_window_max_inner_size) !== JSON.stringify([900, 620])
  || contracts.rules?.onboarding_window_resizable !== false
  || contracts.rules?.onboarding_window_maximizable !== false
  || contracts.rules?.onboarding_window_fixed_size !== true
  || contracts.rules?.main_window_native_decorations !== false
  || contracts.rules?.main_window_custom_chrome_required !== true
  || contracts.rules?.main_window_custom_chrome_edge_to_edge !== true
  || contracts.rules?.main_window_double_frame_forbidden !== true
  || contracts.rules?.main_window_custom_drag_region_required !== true
  || contracts.rules?.onboarding_full_page_layout_required !== true
  || contracts.rules?.onboarding_centered_floating_shell_forbidden !== true
  || contracts.rules?.onboarding_modal_or_dialog_shell_forbidden !== true
  || contracts.rules?.onboarding_large_empty_surrounding_canvas_forbidden !== true
  || JSON.stringify(contracts.rules?.main_window_custom_controls) !== JSON.stringify(["minimize", "close"])) {
  throw new Error("LB-016 machine contract does not freeze Local Bridge success/fixed-window semantics");
}
if (!lb016.required_artifacts.includes("fixed 900x620 non-resizable non-maximizable main window")) throw new Error("LB-016 fixed-window artifact is not frozen");
if (!lb016.required_artifacts.includes("single edge-to-edge custom window chrome with native decorations disabled")) throw new Error("LB-016 single custom chrome artifact is not frozen");
if (!lb016.required_artifacts.includes("full-page onboarding layout using the fixed custom-chrome content area")) throw new Error("LB-016 full-page onboarding artifact is not frozen");
for (const required of [
  `screen 6 success message is ${exactSuccessCopy}`,
  "screens 4 and 5 use Local Bridge as the user-facing connector term",
  "main window is fixed to 900x620 with minimum and maximum 900x620 resizable false and maximizable false",
  "native window decorations are disabled and exactly one edge-to-edge custom chrome provides drag minimize and close without maximize or double frame",
  "onboarding uses the full fixed client content area without a centered floating card modal shell or large empty surrounding canvas",
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

for (const marker of [
  ".inner_size(900.0, 620.0)",
  ".min_inner_size(900.0, 620.0)",
  ".max_inner_size(900.0, 620.0)",
  ".resizable(false)",
  ".maximizable(false)",
  ".decorations(false)",
]) if (!tray.includes(marker)) throw new Error(`LB-016 fixed/custom window contract missing: ${marker}`);
for (const marker of ["MAIN_WINDOW_PHYSICAL_WIDTH", "MAIN_WINDOW_PHYSICAL_HEIGHT", "window.set_size(physical)?", "window.set_zoom(1.0 / scale)?", "enforce_main_window_metrics"])
  if (tray.includes(marker)) throw new Error(`LB-016 stale physical-pixel DPI compensation remains: ${marker}`);
if (tray.includes(".resizable(true)") || tray.includes(".maximizable(true)")) throw new Error("LB-016 main window remains user-resizable/maximizable");
for (const marker of ["WindowEvent::ScaleFactorChanged", "sync_main_webview_to_client(window.app_handle(), client_size)", "let logical_width = f64::from(physical.width) / scale", "let logical_height = f64::from(physical.height) / scale"])
  if (!main.includes(marker)) throw new Error(`LB-016 logical-DIP fixed-window assertion missing: ${marker}`);
for (const marker of ["enforce_main_window_metrics(window.app_handle())", "native physical client is", "set_zoom(1.0 / scale)"])
  if (main.includes(marker)) throw new Error(`LB-016 stale physical-pixel DPI compensation remains in runtime E2E: ${marker}`);
if (main.includes("WindowEvent::Resized") || /\.maximize\(|\.unmaximize\(|\.set_size\(/.test(main)) throw new Error("LB-016 production/debug main contains forbidden resize/maximize behavior");
if (!app.includes('import { WindowChrome } from "./components/WindowChrome"')
  || (app.match(/<WindowChrome>/g) ?? []).length !== 1
  || (app.match(/<\/WindowChrome>/g) ?? []).length !== 1) throw new Error("LB-016 does not production-compose exactly one shared custom window chrome");
if (!app.includes("!onboarding.complete ? <Onboarding") || !app.includes(": <Dashboard />")) throw new Error("LB-016 first-run flow is not composed inside the shared custom chrome ahead of dashboard");
for (const marker of ["getCurrentWindow", "startDragging()", "minimize()", "close()", 'aria-label="最小化"', 'aria-label="关闭"']) if (!chrome.includes(marker)) throw new Error(`LB-016 custom titlebar behavior missing: ${marker}`);
if (/maximize|toggleMaximize/i.test(chrome)) throw new Error("LB-016 custom titlebar exposes forbidden maximize behavior");
const compactSharedCss = sharedCss.replace(/\s+/g, "");
const chromeRule = compactSharedCss.match(/\.window-chrome\{([^}]*)\}/)?.[1] ?? "";
for (const marker of ["position:fixed", "inset:0", "width:100%", "height:100%", "overflow:hidden"]) if (!chromeRule.includes(marker)) throw new Error(`LB-016 custom chrome is not edge-to-edge: ${marker}`);
const exactWindowPermissions = ["core:window:allow-start-dragging", "core:window:allow-minimize", "core:window:allow-close"];
if (windowCapability.identifier !== "window-chrome"
  || JSON.stringify(windowCapability.windows) !== JSON.stringify(["main"])
  || JSON.stringify(windowCapability.permissions) !== JSON.stringify(exactWindowPermissions)) throw new Error("LB-016 custom chrome capability is not exact least privilege");
if (windowCapability.permissions.some((permission) => /maximize|resize|decorations|set-size/i.test(permission))) throw new Error("LB-016 custom chrome capability grants forbidden maximize/window mutation permission");
if (!/\.onboarding-shell\{[^}]*width:100%[^}]*height:100%[^}]*min-height:0/is.test(onboardingCss.replace(/\s+/g, ""))
  || /\.onboarding-shell\{[^}]*(?:100dvh|100vh)/is.test(onboardingCss.replace(/\s+/g, ""))) throw new Error("LB-016 onboarding is not constrained to the fixed custom-chrome content area");
if (frame.includes('className="onboarding-card"') || /\.onboarding-card\b/.test(onboardingCss)) throw new Error("LB-016 onboarding still uses a centered floating card shell instead of a full-page layout");
if (!frame.includes('className="onboarding-page"')) throw new Error("LB-016 onboarding does not expose the frozen full-page root");
const onboardingPageRule = onboardingCss.replace(/\s+/g, "").match(/\.onboarding-page\{([^}]*)\}/)?.[1] ?? "";
for (const marker of ["width:100%", "height:100%"] ) if (!onboardingPageRule.includes(marker)) throw new Error(`LB-016 full-page onboarding root does not fill the content area: ${marker}`);
for (const forbidden of ["box-shadow:", "border-radius:"]) if (onboardingPageRule.includes(forbidden)) throw new Error(`LB-016 full-page onboarding root still looks like a floating dialog: ${forbidden}`);
if (existsSync("tests/e2e/onboarding/resize_runtime_e2e.mjs")) throw new Error("LB-016 obsolete resizable/maximize runtime E2E still exists");
for (const marker of ["tauri.cmd dev --no-watch", "LOCALBRIDGE_FIXED_WINDOW_E2E_VIEW", "CARGO_TARGET_DIR", "LB016_FIXED_WINDOW_E2E=PASS", "logical_fixed=900x620", "native_dpi_scaling=true", "maximizable=false", "single_custom_chrome=true"]) if (!fixedWindowE2e.includes(marker)) throw new Error(`LB-016 real fixed-window E2E runner missing: ${marker}`);
if (/\.maximize\(|\.unmaximize\(|\.set_size\(|LOCALBRIDGE_RESIZE_E2E_VIEW|LB016_REAL_RESIZE_E2E/.test(fixedWindowE2e)) throw new Error("LB-016 fixed-window E2E runner contains obsolete resize/maximize semantics");
for (const marker of ["FixedWindowE2eMetricsSink", "fixed_window_e2e_report", "cfg(debug_assertions)", "cfg(not(debug_assertions))", "localbridge_invoke_handler![]"]) if (!lib.includes(marker)) throw new Error(`LB-016 debug-only fixed-window IPC contract missing: ${marker}`);
if (/ResizeE2eMetricsSink|resize_e2e_report/.test(lib)) throw new Error("LB-016 obsolete resize E2E IPC remains registered");
for (const marker of ["LOCALBRIDGE_FIXED_WINDOW_E2E_VIEW", "inner_size()", "scale_factor()", "is_resizable()", "is_maximizable()", "is_decorated()", "window-chrome", "chrome_count", "is_minimized()", "unminimize()", "is_visible()", "LB016_FIXED_WINDOW_E2E=PASS"]) if (!main.includes(marker)) throw new Error(`LB-016 live fixed-window Tauri/WebView assertion missing: ${marker}`);
const completionCalls = [...onboarding.matchAll(/onboardingApi\.complete\(\)/g)];
const finishStart = onboarding.indexOf("const finish = async () =>");
const screenStart = onboarding.indexOf("if (step === 1)", finishStart);
if (completionCalls.length !== 1 || finishStart < 0 || screenStart <= finishStart
  || completionCalls[0].index <= finishStart || completionCalls[0].index >= screenStart) throw new Error("LB-016 completion is not restricted to the explicit finish handler");
if (!backend.includes("if !current.readiness.all_ready()") || !backend.includes("data.settings.onboarding_complete = true")) throw new Error("LB-016 backend completion does not re-check readiness before persistence");

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

const fixedWindowAuth = auth.records.find((candidate) => candidate.authorization_id === "EXEC-PREAUTH-LB016-010");
const expectedFixedWindowScope = [
  "src-tauri/src/main.rs",
  "src-tauri/src/lib.rs",
  "scripts/authorization-records/LB-016.json",
];
if (!fixedWindowAuth || JSON.stringify(fixedWindowAuth.scope) !== JSON.stringify(expectedFixedWindowScope)
  || fixedWindowAuth.user_audit_status !== "PENDING"
  || fixedWindowAuth.does_not_expand_future_pr_writable_paths !== true
  || !fixedWindowAuth.evidence_ref.includes("2026-08-13")
  || !fixedWindowAuth.evidence_ref.includes("2eb11fc")
  || !fixedWindowAuth.evidence_ref.includes("fixed 900x620")) throw new Error("LB-016 fixed-window runtime preauthorization invalid");

console.log("LB016_CONTRACT=PASS six_screens=true no_screen7=true key_secure=true native_folder_picker=true fixed_connector_deeplink=true typed_verified_endpoint=true frontend_endpoint_derivation=false fake_chatgpt_state=false copy_feedback_reserved=true shared_buttons=true checks=3 confirm_gated=true exact_success=true local_bridge_term=true fixed_window=900x620 resizable=false maximizable=false native_decorations=false single_custom_chrome=true edge_to_edge=true controls=drag,minimize,close maximize=false real_fixed_window_e2e_required=true resize_e2e_forbidden=true fixed_window_preauth=EXEC-PREAUTH-LB016-010");
