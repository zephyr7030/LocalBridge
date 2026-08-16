import { readFileSync } from "node:fs";

const app = readFileSync("src/App.tsx", "utf8");
const chrome = readFileSync("src/components/WindowChrome.tsx", "utf8");
const css = readFileSync("src/styles.css", "utf8");
const onboardingCss = readFileSync("src/features/onboarding/onboarding.css", "utf8");
const tray = readFileSync("src-tauri/src/tray/mod.rs", "utf8");
const capability = JSON.parse(readFileSync("src-tauri/capabilities/window-chrome.json", "utf8"));
const compactCss = css.replace(/\s+/g, "");
const compactOnboarding = onboardingCss.replace(/\s+/g, "");

for (const marker of [
  ".inner_size(780.0, 620.0)",
  ".min_inner_size(780.0, 620.0)",
  ".max_inner_size(780.0, 620.0)",
  ".resizable(false)",
  ".maximizable(false)",
  ".decorations(false)",
]) if (!tray.includes(marker)) throw new Error(`LB-015 native fixed-window baseline missing: ${marker}`);
const existingWindowBranch = tray.slice(tray.indexOf("if let Some(window)"), tray.indexOf("let window ="));
if (existingWindowBranch.includes(".center()") || existingWindowBranch.includes("window.center()")) throw new Error("LB-015 reopening an existing hidden window forcibly recenters it");
const newWindowStart = tray.indexOf("let window =");
const newWindowBranch = tray.slice(newWindowStart, tray.indexOf("Ok(window)", newWindowStart));
if (!newWindowBranch.includes("window.center()?;")) throw new Error("LB-015 first-created native window is not explicitly centered");
for (const marker of [
  "MAIN_WINDOW_PHYSICAL_WIDTH",
  "MAIN_WINDOW_PHYSICAL_HEIGHT",
  "PhysicalSize::new(MAIN_WINDOW_PHYSICAL_WIDTH, MAIN_WINDOW_PHYSICAL_HEIGHT)",
  "window.set_size(physical)?",
  "window.set_zoom(1.0 / scale)?",
]) if (tray.includes(marker)) throw new Error(`LB-015 stale physical-pixel DPI compensation remains: ${marker}`);

if ((app.match(/<WindowChrome>/g) ?? []).length !== 1 || (app.match(/<\/WindowChrome>/g) ?? []).length !== 1) {
  throw new Error("LB-015 App must compose exactly one shared WindowChrome around all views");
}
for (const marker of ["getCurrentWindow", "startDragging()", "minimize()", "close()", 'aria-label="最小化"', 'aria-label="关闭"']) {
  if (!chrome.includes(marker)) throw new Error(`LB-015 custom titlebar behavior missing: ${marker}`);
}
if (/maximize|toggleMaximize/i.test(chrome)) throw new Error("LB-015 custom titlebar exposes forbidden maximize behavior");
const exactWindowPermissions = [
  "core:window:allow-start-dragging",
  "core:window:allow-minimize",
  "core:window:allow-close",
];
if (capability.identifier !== "window-chrome"
  || JSON.stringify(capability.windows) !== JSON.stringify(["main"])
  || JSON.stringify(capability.permissions) !== JSON.stringify(exactWindowPermissions)) {
  throw new Error("LB-015 custom titlebar Tauri capability is not exact least-privilege");
}
if (capability.permissions.some((permission) => /maximize|resize|decorations|set-size/i.test(permission))) {
  throw new Error("LB-015 custom titlebar capability grants forbidden window mutation/maximize permission");
}

const chromeRule = compactCss.match(/\.window-chrome\{([^}]*)\}/)?.[1] ?? "";
for (const token of ["position:fixed", "inset:0", "width:100%", "height:100%", "overflow:hidden", "border:1pxsolidrgba(29,29,31,.22)"]) {
  if (!chromeRule.includes(token)) throw new Error(`LB-015 edge-to-edge custom chrome missing: ${token}`);
}
const contentRule = compactCss.match(/\.window-content\{([^}]*)\}/)?.[1] ?? "";
for (const token of ["min-width:0", "min-height:0", "overflow:auto"]) {
  if (!contentRule.includes(token)) throw new Error(`LB-015 chrome content sizing missing: ${token}`);
}
const shellRule = compactCss.match(/\.shell\{([^}]*)\}/)?.[1] ?? "";
if (!shellRule.includes("min-height:100%") || /100dvh|100vh/.test(shellRule)) throw new Error("LB-015 Dashboard is not constrained to chrome content area");
const onboardingRule = compactOnboarding.match(/\.onboarding-shell\{([^}]*)\}/)?.[1] ?? "";
for (const token of ["width:100%", "height:100%", "min-height:0"]) if (!onboardingRule.includes(token)) throw new Error(`LB-015 onboarding fixed-content sizing missing: ${token}`);
if (/100dvh|100vh/.test(onboardingRule)) throw new Error("LB-015 onboarding still escapes chrome content area");

console.log("LB015_WINDOW_CHROME=PASS logical_fixed=780x620 native_dpi_scaling=true native_decorations=false single_custom_chrome=true edge_to_edge=true drag=true minimize=true close=true maximize=false least_privilege_capability=true dashboard_fit=true onboarding_fit=true");
