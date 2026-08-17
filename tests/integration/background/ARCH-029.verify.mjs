import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const ruleId = process.env.LOCALBRIDGE_ARCH_RULE_ID;
if (ruleId && ruleId !== "ARCH-029") throw new Error(`ARCH-029 invoked as ${ruleId}`);
const contracts = JSON.parse(readFileSync("PR_CONTRACTS.json", "utf8"));
const progress = JSON.parse(readFileSync("PR_INDEX.json", "utf8"));
for (const [key, expected] of Object.entries({
  ui_panel_base_color_difference_required: false,
  ui_flat_primary_surface_allowed: true,
  ui_fake_elevation_through_near_invisible_surface_treatment_forbidden: true,
  dashboard_permission_mode_row_forbidden: true,
  dashboard_permission_mode_read_only_row_required: false,
  dashboard_permission_mode_status_dot_forbidden: true,
  dashboard_admin_privilege_status_read_only: true,
  onboarding_screen_4_new_connector_action_label: "打开新建插件页",
  onboarding_screen_4_new_connector_action_before_information_rows: true,
  onboarding_screen_4_information_label_column_aligned: true,
  onboarding_screen_4_copy_action_right_edge_aligned: true,
  tray_icon_ico: "assets/icons/localbridge-tray.ico",
  tray_icon_small_frame_simplified_design_required: false,
  tray_icon_runtime_resampling_forbidden: true,
  tray_icon_large_brand_art_direct_use_forbidden: true,
  windows_icon_master_png: "assets/icons/localbridge.png",
  windows_icon_master_png_sha256: "710690f2d70e3c69f13db9d4eaebc0bef5c80561c74acc7bc5a401c15c16e55a",
  windows_taskbar_icon_ico: "assets/icons/localbridge.ico",
  windows_taskbar_icon_sha256: "c995d6af01ebc5031950eb9ea6415b58671b31f84ed6b55baabe80ea51e33f78",
  windows_taskbar_icon_master_png_derivation_required: true,
  tray_icon_master_png_derivation_required: true,
  tray_icon_new_authored_graphics_forbidden: true,
  tray_icon_derived_sha256: "5a9fce6e80050c9ce1620b8e28767052ece2536213ee04fe532596d3f4ec811d",
  dashboard_project_scope_display_source: "permission_mode",
  dashboard_project_scope_display_privilege_state_independent: true,
  dashboard_project_switch_blocked_in_admin_mode: true,
})) if (JSON.stringify(contracts.rules?.[key]) !== JSON.stringify(expected)) throw new Error(`ARCH-029 contract drift: ${key}`);
if (JSON.stringify(contracts.rules.tray_icon_required_frame_sizes) !== JSON.stringify([16,20,24,32,48])) throw new Error("ARCH-029 tray frame inventory drift");
if (JSON.stringify(contracts.rules.tray_icon_exact_dpi_frame_mapping) !== JSON.stringify({"1.0":16,"1.25":20,"1.5":24,"2.0":32})) throw new Error("ARCH-029 tray DPI mapping drift");
if (JSON.stringify(contracts.rules.tray_icon_allowed_processing) !== JSON.stringify(["crop","downscale","ico-packaging"])) throw new Error("ARCH-029 tray allowed processing drift");
if (JSON.stringify(contracts.rules.tray_icon_source_crop_rect) !== JSON.stringify({x:90,y:400,width:390,height:390})) throw new Error("ARCH-029 tray source crop drift");
if (JSON.stringify(contracts.rules.dashboard_project_scope_by_permission_mode) !== JSON.stringify({edit:"active_workspace_path",full:"active_workspace_path",admin:"全目录访问"})) throw new Error("ARCH-029 Dashboard project scope mapping drift");

const app = readFileSync("src/App.tsx", "utf8");
const css = readFileSync("src/styles.css", "utf8").replace(/\s+/g, "");
const onboarding = readFileSync("src/features/onboarding/Onboarding.tsx", "utf8");
const onboardingCss = readFileSync("src/features/onboarding/onboarding.css", "utf8").replace(/\s+/g, "");
const tray = readFileSync("src-tauri/src/tray/mod.rs", "utf8");
const trayDeriver = readFileSync("scripts/icons/derive-tray-icon.ps1", "utf8");
const lb015Passed = progress.prs?.find?.((pr) => pr.id === "LB-015")?.status === "PASS";
if (lb015Passed && (app.includes('>权限模式</span>') || app.includes('accessText[projection.permission]'))) throw new Error("ARCH-029 Dashboard PermissionMode row survived LB-015");
if (!app.includes('const adminModeFullAccess = projection?.permission === "admin"') || app.includes('projection?.permission === "admin" && projection?.privilege === "active"')) throw new Error("ARCH-029 Dashboard project scope still depends on PrivilegeState");
if (!app.includes('adminModeFullAccess ? "全目录访问" : activeProject?.path ?? "未选择项目"') || !app.includes('if (adminModeFullAccess)')) throw new Error("ARCH-029 Dashboard project scope/switch is not PermissionMode-bound");
const dashboardBeforeSettings = app.slice(0, app.indexOf('view === "settings"'));
if (dashboardBeforeSettings.includes('permission-mode-value"><ServiceStatusDot')) throw new Error("ARCH-029 PermissionMode row regained status dot");
const card = css.match(/\.card\{([^}]*)\}/)?.[1] ?? "";
for (const marker of ["background:transparent","border:0","border-radius:0","box-shadow:none"]) if (!card.includes(marker)) throw new Error(`ARCH-029 flat surface drift: ${marker}`);
const action = onboarding.indexOf('>打开新建插件页</button>');
const rows = onboarding.indexOf('<div className="onboarding-plugin-info">');
if (action < 0 || rows < 0 || action > rows || !onboarding.includes('打开新建插件页后，选择隧道并选择刚刚添加的Tunel，创建插件')) throw new Error("ARCH-029 onboarding Screen4 action/order drift");
if (!onboardingCss.includes(".onboarding-info-row{display:grid;grid-template-columns:6emminmax(0,1fr)max-content") || !onboardingCss.includes(".onboarding-copy-action{justify-self:end}")) throw new Error("ARCH-029 onboarding identity alignment drift");
if (!tray.includes('localbridge-tray.ico') || tray.includes('include_bytes!("../../../assets/icons/localbridge.ico")')) throw new Error("ARCH-029 tray still uses large brand ICO");
const ico = readFileSync("assets/icons/localbridge-tray.ico");
if (createHash("sha256").update(readFileSync("assets/icons/localbridge.png")).digest("hex") !== "710690f2d70e3c69f13db9d4eaebc0bef5c80561c74acc7bc5a401c15c16e55a") throw new Error("ARCH-029 master PNG drift");
if (createHash("sha256").update(readFileSync("assets/icons/localbridge.ico")).digest("hex") !== "c995d6af01ebc5031950eb9ea6415b58671b31f84ed6b55baabe80ea51e33f78") throw new Error("ARCH-029 accepted taskbar/left-bottom icon drift");
if (createHash("sha256").update(ico).digest("hex") !== "5a9fce6e80050c9ce1620b8e28767052ece2536213ee04fe532596d3f4ec811d") throw new Error("ARCH-029 PNG-derived tray asset drift");
for (const marker of ["assets/icons/localbridge.png", "$CropX = 90", "$CropY = 400", "$CropWidth = 390", "$CropHeight = 390", "$FrameSizes = @(16, 20, 24, 32, 48)", ".DrawImage($sourceBitmap", "HighQualityBicubic"]) if (!trayDeriver.includes(marker)) throw new Error(`ARCH-029 tray derivation marker missing: ${marker}`);
if (/FillRectangle|FillEllipse|DrawString|DrawIcon|DrawLine|DrawPolygon|GraphicsPath/i.test(trayDeriver)) throw new Error("ARCH-029 tray derivation contains newly authored graphics operations");
const count = ico.readUInt16LE(4); const sizes = [];
for (let i=0;i<count;i+=1){const raw=ico[6+i*16];sizes.push(raw===0?256:raw);}
if (JSON.stringify(sizes) !== JSON.stringify([16,20,24,32,48])) throw new Error(`ARCH-029 tray frame sizes drift: ${sizes.join(",")}`);
console.log("ARCH-029_VERIFY=PASS flat_surface=true dashboard_permission_mode=read_only_no_dot onboarding_new_connector=aligned icon_master_png=true taskbar_frozen_clear=true tray_png_crop_downscale_only=true tray_frames=16,20,24,32,48 exact_dpi=true");
