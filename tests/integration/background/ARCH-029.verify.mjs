import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const ruleId = process.env.LOCALBRIDGE_ARCH_RULE_ID;
if (ruleId && ruleId !== "ARCH-029") throw new Error(`ARCH-029 invoked as ${ruleId}`);
const contracts = JSON.parse(readFileSync("PR_CONTRACTS.json", "utf8"));
for (const [key, expected] of Object.entries({
  ui_panel_base_color_difference_required: false,
  ui_flat_primary_surface_allowed: true,
  ui_fake_elevation_through_near_invisible_surface_treatment_forbidden: true,
  dashboard_permission_mode_row_forbidden: false,
  dashboard_permission_mode_read_only_row_required: true,
  dashboard_permission_mode_row_label: "权限模式",
  dashboard_permission_mode_status_dot_forbidden: true,
  onboarding_screen_4_new_connector_action_label: "打开新建插件页",
  onboarding_screen_4_new_connector_action_before_information_rows: true,
  onboarding_screen_4_information_label_column_aligned: true,
  onboarding_screen_4_copy_action_right_edge_aligned: true,
  tray_icon_ico: "assets/icons/localbridge-tray.ico",
  tray_icon_small_frame_simplified_design_required: true,
  tray_icon_runtime_resampling_forbidden: true,
  tray_icon_large_brand_art_direct_use_forbidden: true,
})) if (JSON.stringify(contracts.rules?.[key]) !== JSON.stringify(expected)) throw new Error(`ARCH-029 contract drift: ${key}`);
if (JSON.stringify(contracts.rules.dashboard_permission_mode_row_values) !== JSON.stringify(["编辑模式","完整模式","管理员模式"])) throw new Error("ARCH-029 permission-mode value inventory drift");
if (JSON.stringify(contracts.rules.tray_icon_required_frame_sizes) !== JSON.stringify([16,20,24,32,48])) throw new Error("ARCH-029 tray frame inventory drift");
if (JSON.stringify(contracts.rules.tray_icon_exact_dpi_frame_mapping) !== JSON.stringify({"1.0":16,"1.25":20,"1.5":24,"2.0":32})) throw new Error("ARCH-029 tray DPI mapping drift");

const app = readFileSync("src/App.tsx", "utf8");
const css = readFileSync("src/styles.css", "utf8").replace(/\s+/g, "");
const onboarding = readFileSync("src/features/onboarding/Onboarding.tsx", "utf8");
const onboardingCss = readFileSync("src/features/onboarding/onboarding.css", "utf8").replace(/\s+/g, "");
const tray = readFileSync("src-tauri/src/tray/mod.rs", "utf8");
if (!app.includes('>权限模式</span>') || !app.includes('accessText[projection.permission]') || app.includes('>管理员权限</span>')) throw new Error("ARCH-029 Dashboard PermissionMode projection drift");
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
if (createHash("sha256").update(ico).digest("hex") !== "1cbf7251fc08366b4107b633d0145886526ebec45f4c9d0d01c55c6e2683a06c") throw new Error("ARCH-029 tray asset drift");
const count = ico.readUInt16LE(4); const sizes = [];
for (let i=0;i<count;i+=1){const raw=ico[6+i*16];sizes.push(raw===0?256:raw);}
if (JSON.stringify(sizes) !== JSON.stringify([16,20,24,32,48])) throw new Error(`ARCH-029 tray frame sizes drift: ${sizes.join(",")}`);
console.log("ARCH-029_VERIFY=PASS flat_surface=true dashboard_permission_mode=read_only_no_dot onboarding_new_connector=aligned tray_frames=16,20,24,32,48 exact_dpi=true");
