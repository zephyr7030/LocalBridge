import { existsSync, readFileSync } from "node:fs";

const read = (path) => readFileSync(path, "utf8");
const contracts = JSON.parse(read("PR_CONTRACTS.json"));
const auth = JSON.parse(read("scripts/authorization-records/LB-016.json"));
const onboarding = read("src/features/onboarding/Onboarding.tsx");
const api = read("src/features/onboarding/api.ts");
const frame = read("src/components/WizardFrame.tsx");
const readiness = read("src/components/ReadinessCheck.tsx");
const serviceDot = read("src/components/ServiceStatusDot.tsx");
const presentation = read("src/presentation.ts");
const bridge = read("src/bridge.ts");
const app = read("src/App.tsx");
const onboardingCss = read("src/features/onboarding/onboarding.css");
const sharedCss = read("src/styles.css");
const backend = read("src-tauri/src/commands/onboarding.rs");
const uiBackend = read("src-tauri/src/commands/ui.rs");
const tray = read("src-tauri/src/tray/mod.rs");
const lib = read("src-tauri/src/lib.rs");
const capability = JSON.parse(read("src-tauri/capabilities/window-chrome.json"));
const fixedWindowE2e = read("tests/e2e/onboarding/fixed_window_runtime_e2e.mjs");

const lb016 = contracts.prs?.["LB-016"];
if (!lb016) throw new Error("LB-016 machine contract missing");
const exactSuccess = "配置完成，在插件中选择刚刚添加的Local Bridge试试吧";
const exactPluginHint = "打开插件管理页后，选择隧道并选择刚刚添加的Tunel，创建插件";
const pluginsUrl = "https://chatgpt.com/plugins#settings/Plugins";
const managementUrl = "https://chatgpt.com/plugins#settings/Connectors?create-connector=true&redirectAfter=%2Fplugins";

// Machine authority: current contract is five-screen only.
if (contracts.schema_version !== 19
  || contracts.rules?.onboarding_screen_count !== 5
  || contracts.rules?.onboarding_screen_6_forbidden !== true
  || contracts.rules?.onboarding_screen_5_confirm_requires_all_green !== true
  || contracts.rules?.onboarding_screen_5_auto_advance_forbidden !== true
  || contracts.rules?.onboarding_screen_5_success_message !== exactSuccess
  || JSON.stringify(contracts.rules?.onboarding_back_path_required_from_screens) !== JSON.stringify([2, 3, 4, 5])
  || contracts.rules?.onboarding_failure_backtracking_required !== true) {
  throw new Error("LB-016 five-screen/backtracking machine contract drifted");
}
if (!lb016.required_artifacts.includes("five-screen onboarding flow") || lb016.required_artifacts.includes("six-screen onboarding flow")) throw new Error("LB-016 artifact still permits six screens");
for (const required of [
  "onboarding has exactly five screens",
  "onboarding has no sixth screen",
  "screen 4 provides an explicit back action to screen 3 and a continue action to screen 5",
  "screens 2 3 4 and 5 each provide an explicit back path and save start or configuration failure never traps the user",
  "screen 5 status dots map Ready to green Starting to amber Fault to red and Unknown to gray using the shared typed service-status source",
  `screen 5 success message is ${exactSuccess}`,
  `screen 4 shows ${exactPluginHint} beneath the plugin-settings action`,
]) if (!lb016.required_tests.includes(required)) throw new Error(`LB-016 current test contract missing: ${required}`);

// Exactly five production screens, no obsolete confirmation or sixth-screen path.
for (const marker of [
  'step={1} title="简单设置 即可开始"',
  'step={2} title="OpenAI 设置"',
  'step={3} title="项目与权限"',
  'step={4} title="创建自定义插件"',
  'step={5} title="启动检查"',
]) if (!onboarding.includes(marker)) throw new Error(`LB-016 production screen missing: ${marker}`);
if (!frame.includes("{step} / 5")) throw new Error("LB-016 WizardFrame is not /5");
for (const forbidden of ["Local Bridge 使用确认", "step={6}", "step === 6", "setStep(6)", "{step} / 6"]) if (`${onboarding}\n${frame}`.includes(forbidden)) throw new Error(`LB-016 obsolete six-screen production returned: ${forbidden}`);

// Screens 2-5 all have an explicit backward edge; failures restore navigation.
for (const marker of ["setStep(1)", "setStep(2)", "setStep(3)", "setStep(4)"]) if (!onboarding.includes(marker)) throw new Error(`LB-016 back path missing: ${marker}`);
if (!onboarding.includes("finally") || !onboarding.includes("setPreparingProject(false)")) throw new Error("LB-016 Screen3 failure can leave navigation locked");

// OpenAI secret and folder-selection boundaries remain intact.
for (const label of [">Tunnel ID</label>", ">Runtime API Key</label>"]) if (!onboarding.includes(label)) throw new Error(`LB-016 OpenAI field missing: ${label}`);
if (!onboarding.includes('type="password"') || !onboarding.includes('autoComplete="off"') || !onboarding.includes('setRuntimeKey("")')) throw new Error("LB-016 Runtime API Key UI safety regressed");
for (const forbidden of ["localStorage", "sessionStorage", "indexedDB"]) if (`${onboarding}\n${api}`.includes(forbidden)) throw new Error(`LB-016 browser secret storage forbidden: ${forbidden}`);
if (!onboarding.includes("onboardingApi.chooseWorkspaceFolder()") || !api.includes('invoke<string | null>("choose_onboarding_workspace_folder")')) throw new Error("LB-016 native folder picker frontend path missing");
for (const marker of ["SHBrowseForFolderW", "SHGetPathFromIDListW", "BIF_RETURNONLYFSDIRS"]) if (!backend.includes(marker)) throw new Error(`LB-016 native Windows folder picker backend missing: ${marker}`);

// Screen3 spacing and logical colors.
const compactOnboardingCss = onboardingCss.replace(/\s+/g, "");
const permissionRule = compactOnboardingCss.match(/\.onboarding-permission\{([^}]*)\}/)?.[1] ?? "";
for (const marker of ["height:auto", "padding:15px16px", "gap:7px", "white-space:normal"]) if (!permissionRule.includes(marker)) throw new Error(`LB-016 wrap-safe permission structure missing: ${marker}`);
if (/min-height:/.test(permissionRule)) throw new Error("LB-016 permission button fixed/min height can squeeze text");
if (!onboarding.includes('mode === "admin" ? "admin-choice"')) throw new Error("LB-016 onboarding admin logical class missing");
const compactSharedCss = sharedCss.replace(/\s+/g, "");
for (const marker of ["--accent:#0071e3", ".choice.selected{background:var(--accent)", ".choice.admin-choice.selected{background:var(--admin-accent)"]) if (!compactSharedCss.includes(marker)) throw new Error(`LB-016 shared blue/admin styling missing: ${marker}`);
if (/\.choice\.selected\{[^}]*background:#1d1d1f/i.test(sharedCss)) throw new Error("LB-016 ordinary selected accent regressed to black");

// Screen3 must start and reach real readiness before Screen4. No later unique start edge.
const saveStart = onboarding.indexOf("const saveProjectAndPermission = async () =>");
const copyStart = onboarding.indexOf("const copyScreen4Value", saveStart);
const saveFlow = saveStart >= 0 && copyStart > saveStart ? onboarding.slice(saveStart, copyStart) : "";
const startIndex = saveFlow.indexOf("onboardingApi.startProject(selectedProject)");
const readyIndex = saveFlow.indexOf("next.readiness.localEnvironment && next.readiness.codingService && next.readiness.openaiTunnel");
const step4Index = saveFlow.indexOf("setStep(4)");
if (startIndex < 0 || readyIndex <= startIndex || step4Index <= readyIndex || !saveFlow.includes("READY_POLL_ATTEMPTS") || !saveFlow.includes("if (!readyState) throw new Error")) throw new Error("LB-016 Screen3 runtime-ready-before-Screen4 edge regressed");
if ((onboarding.match(/onboardingApi\.startProject\(/g) ?? []).length !== 1) throw new Error("LB-016 runtime startup edge was duplicated/deferred");

// Screen4 fixed-browser and exact content contract.
if (!backend.includes(`CHATGPT_PLUGINS_SETTINGS_URL: &str = "${pluginsUrl}"`) || !backend.includes(`CHATGPT_CUSTOM_CONNECTOR_URL: &str = "${managementUrl}"`)) throw new Error("LB-016 fixed ChatGPT URL constants missing");
for (const marker of ["open_allowlisted_url(CHATGPT_PLUGINS_SETTINGS_URL)", "open_allowlisted_url(CHATGPT_CUSTOM_CONNECTOR_URL)", "ShellExecuteW"]) if (!backend.includes(marker)) throw new Error(`LB-016 system-browser allowlist adapter missing: ${marker}`);
if (`${onboarding}\n${api}`.includes(pluginsUrl) || `${onboarding}\n${api}`.includes(managementUrl)) throw new Error("LB-016 frontend embeds ChatGPT URL literal");
if (/WebviewWindowBuilder|WebviewUrl|window\.open/.test(`${backend}\n${onboarding}\n${api}`)) throw new Error("LB-016 forbidden WebView/arbitrary browser path returned");
if (!onboarding.includes(exactPluginHint)) throw new Error("LB-016 exact Tunel guidance missing");
if (!compactOnboardingCss.includes(".onboarding-plugin-top-action,.onboarding-plugin-management{display:flex;justify-content:flex-start}")) throw new Error("LB-016 Screen4 browser actions are not left-aligned");
const screen4Start = onboarding.indexOf("if (step === 4)");
const screen5Start = onboarding.indexOf("if (step === 5)", screen4Start);
const screen4 = screen4Start >= 0 && screen5Start > screen4Start ? onboarding.slice(screen4Start, screen5Start) : "";
if ((screen4.match(/onboarding-info-row/g) ?? []).length !== 2 || screen4.includes("本地服务")) throw new Error("LB-016 Screen4 must have exactly two information rows and no 本地服务");
for (const marker of [">名称</span>", ">Local Bridge</span>", ">Tunnel ID</span>", "state.tunnelId", 'copyScreen4Value("name", "Local Bridge")', 'copyScreen4Value("tunnel", state.tunnelId)', "setStep(3)", "setStep(5)"]) if (!screen4.includes(marker)) throw new Error(`LB-016 Screen4 row/navigation contract missing: ${marker}`);
for (const marker of ["tunnelId: string | null", "tunnel_id: Option<String>", "validated_tunnel_id()", "tunnel_id.map(|value| value.expose().to_owned())"]) if (!`${api}\n${backend}`.includes(marker)) throw new Error(`LB-016 persisted Tunnel ID projection missing: ${marker}`);
for (const marker of ["copyTimers", "setCopiedRows", "window.setTimeout(() =>", "3000"]) if (!onboarding.includes(marker)) throw new Error(`LB-016 independent 3s copy feedback missing: ${marker}`);
if (!compactOnboardingCss.includes(".onboarding-copy-action.copied{color:#237a49")) throw new Error("LB-016 copied state is not green/stable");

// Screen5 and Dashboard consume one typed ServiceCode -> ServiceStatusDot mapping.
for (const marker of ["ServiceStatusDot", "serviceVisualState[service]", "data-service-state={state}"]) if (!serviceDot.includes(marker)) throw new Error(`LB-016 shared status-dot component missing: ${marker}`);
for (const marker of ['off: "unknown"', 'starting: "starting"', 'online: "ready"', 'recovering: "starting"', 'fault: "fault"']) if (!presentation.includes(marker)) throw new Error(`LB-016 typed visual-state mapping missing: ${marker}`);
for (const marker of ["localEnvironmentService: ServiceCode", "tunnelService: ServiceCode", "codingService: ServiceCode"]) if (!bridge.includes(marker)) throw new Error(`LB-016 MainProjection typed service missing: ${marker}`);
for (const marker of ["ServiceStatusDot", "projection?.tunnelService ?? null", "projection?.codingService ?? null"]) if (!app.includes(marker)) throw new Error(`LB-016 Dashboard shared dot missing: ${marker}`);
for (const marker of ["ServiceStatusDot", "service={main?.localEnvironmentService ?? null}", "service={main?.codingService ?? null}", "service={main?.tunnelService ?? null}"]) if (!`${readiness}\n${onboarding}`.includes(marker)) throw new Error(`LB-016 onboarding typed readiness missing: ${marker}`);
if (onboardingCss.includes("readiness-dot")) throw new Error("LB-016 stale boolean readiness-dot CSS returned");
for (const marker of [".service-status-dot.status-ready", ".service-status-dot.status-starting", ".service-status-dot.status-fault", ".service-status-dot.status-unknown"]) if (!sharedCss.includes(marker)) throw new Error(`LB-016 shared status color class missing: ${marker}`);

// Screen5 exact completion gate.
const screen5 = screen5Start >= 0 ? onboarding.slice(screen5Start) : "";
if ((screen5.match(/<ReadinessCheck/g) ?? []).length !== 3) throw new Error("LB-016 Screen5 must contain exactly three readiness checks");
for (const marker of ['title="启动检查"', "disabled={!allGreen}", `{allGreen ? <p className=\"onboarding-success\">${exactSuccess}</p> : null}`, "setStep(4)", "void finish()"] ) if (!screen5.includes(marker)) throw new Error(`LB-016 Screen5 gate missing: ${marker}`);
if (/setStep\(6\)|startProject\(/.test(screen5)) throw new Error("LB-016 Screen5 contains forbidden later startup/sixth-screen edge");

// Backend completion remains fail-closed and explicit.
const completeStart = backend.indexOf("pub fn complete_onboarding");
const completeEnd = backend.indexOf("fn project_state", completeStart);
const completeFlow = completeStart >= 0 && completeEnd > completeStart ? backend.slice(completeStart, completeEnd) : "";
for (const marker of ["project_state(&app, &lifecycle)?", "current.readiness.all_ready()", "return Err(", "onboarding_complete = true"]) if (!completeFlow.includes(marker)) throw new Error(`LB-016 backend completion gate missing: ${marker}`);
for (const command of ["get_onboarding_state", "save_onboarding_connection", "open_chatgpt_plugins_settings", "open_chatgpt_custom_connector_settings", "choose_onboarding_workspace_folder", "complete_onboarding"]) if (!lib.includes(`commands::onboarding::${command}`)) throw new Error(`LB-016 command not registered: ${command}`);

// Fixed 900x620 single custom chrome is still mandatory and has a real runtime E2E.
for (const marker of [".inner_size(900.0, 620.0)", ".min_inner_size(900.0, 620.0)", ".max_inner_size(900.0, 620.0)", ".resizable(false)", ".maximizable(false)", ".decorations(false)"]) if (!tray.includes(marker)) throw new Error(`LB-016 fixed native window marker missing: ${marker}`);
if (!Array.isArray(capability.permissions) || !capability.permissions.includes("core:window:allow-start-dragging") || !capability.permissions.includes("core:window:allow-minimize") || !capability.permissions.includes("core:window:allow-close")) throw new Error("LB-016 custom chrome capability incomplete");
if (!fixedWindowE2e.includes("LB016_FIXED_WINDOW_E2E=PASS") || !fixedWindowE2e.includes("logical_fixed=900x620")) throw new Error("LB-016 real fixed-window runtime E2E contract missing");
if (!existsSync("tests/e2e/onboarding/fixed_window_runtime_e2e.mjs")) throw new Error("LB-016 fixed-window runtime E2E file missing");

// New amendment exists; older records remain historical and untouched/non-authoritative.
const latest = auth.records.find((record) => record.authorization_id === "EXEC-PREAUTH-LB016-014");
if (!latest || latest.user_audit_status !== "PENDING" || latest.does_not_expand_future_pr_writable_paths !== true || !latest.evidence_ref.includes("strict 5 screens") || !latest.actions.some((action) => action.includes("same typed MainProjection") && action.includes("shared ServiceStatusDot"))) throw new Error("LB-016 latest five-screen preauthorization invalid");
if (!auth.records.find((record) => record.authorization_id === "EXEC-PREAUTH-LB016-013")) throw new Error("LB-016 historical authorization provenance was rewritten/removed");

console.log("LB016_CONTRACT=PASS five_screens=true no_screen6=true backs=2,3,4,5 screen3_runtime_ready_before_screen4=true screen4_left_actions=true exact_tunel_hint=true two_persisted_rows=true copy=green_3s shared_typed_status=true ready=green starting=amber fault=red unknown=gray accent=#0071e3 admin=amber fixed_window=900x620 latest_preauth=EXEC-PREAUTH-LB016-014");
