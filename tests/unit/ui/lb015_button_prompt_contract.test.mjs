import { readFileSync } from "node:fs";

const css = readFileSync("src/styles.css", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
const chrome = readFileSync("src/components/WindowChrome.tsx", "utf8");
const onboardingCss = readFileSync("src/features/onboarding/onboarding.css", "utf8");
const diagnostics = readFileSync("src/features/diagnostics/Diagnostics.tsx", "utf8");
const ui = `${app}\n${diagnostics}`;

for (const token of [
  "--control-height:40px",
  "--control-radius:10px",
  ".ghost,.secondary,.primary,.choice",
  "border:1px solid transparent",
  ".secondary{background:var(--control-neutral);border-color:var(--control-border)",
  ".ghost{background:#fff;border-color:var(--control-border)",
  ":not(:disabled):hover",
  ":focus-visible",
  ":disabled",
  "cursor:not-allowed",
  "prefers-reduced-motion: reduce",
  ".ghost,.secondary,.primary,.choice{transition:none}",
]) {
  if (!css.includes(token)) throw new Error(`LB-015 button contract missing: ${token}`);
}

if (/\.ghost\s*,\s*\.secondary\s*\{[^}]*background\s*:\s*#fff/i.test(css)) {
  throw new Error("LB-015 secondary and ghost buttons still collapse to the same white-on-white treatment");
}

for (const redundant of [
  "所有状态都在这一行收口。",
  "通常无需调整；仅在需要检查路径时使用。",
  "仅在出现问题时查看。",
]) {
  if (ui.includes(redundant)) throw new Error(`LB-015 redundant helper copy returned: ${redundant}`);
}

if (!app.includes("不会删除项目文件。")) {
  throw new Error("LB-015 minimum-prompt cleanup removed required destructive-action safety copy");
}

const compactCss = css.replace(/\s+/g, "");
if (!compactCss.includes("html,body,#root{width:100%;min-width:0;height:100%;min-height:0}")) throw new Error("LB-015 document/root does not exactly fill the fixed client area");
if (!compactCss.includes("body{margin:0;min-width:320px;height:100%;overflow:hidden;font-size:var(--font-body)}") || !compactCss.includes("#root{height:100%;min-height:0;background:#f5f5f7}")) throw new Error("LB-015 body/root is not bound to the single custom chrome or schema35 body typography");
const shellRule = compactCss.match(/\.shell\{([^}]*)\}/)?.[1] ?? "";
if (!shellRule.includes("width:calc(100%-clamp(32px,6vw,72px))") || !shellRule.includes("max-width:1180px") || !shellRule.includes("min-height:100%")) throw new Error("LB-015 Dashboard shell does not fit the fixed chrome content area");
if (/\.shell\{[^}]*760px/.test(compactCss) || /100dvh|100vh/.test(shellRule)) throw new Error("LB-015 Dashboard escaped the fixed chrome content area");
const compactOnboarding = onboardingCss.replace(/\s+/g, "");
const onboardingRule = compactOnboarding.match(/\.onboarding-shell\{([^}]*)\}/)?.[1] ?? "";
for (const token of ["width:100%", "height:100%", "min-height:0"]) if (!onboardingRule.includes(token)) throw new Error(`LB-015 onboarding fixed-content sizing missing: ${token}`);
if (/100dvh|100vh/.test(onboardingRule)) throw new Error("LB-015 onboarding escaped the fixed chrome content area");
if ((app.match(/<WindowChrome>/g) ?? []).length !== 1 || (app.match(/<\/WindowChrome>/g) ?? []).length !== 1) throw new Error("LB-015 must compose exactly one shared custom chrome");
for (const marker of ["getCurrentWindow", "startDragging()", "minimize()", "close()", 'aria-label="最小化"', 'aria-label="关闭"']) if (!chrome.includes(marker)) throw new Error(`LB-015 custom titlebar behavior missing: ${marker}`);
if (/maximize|toggleMaximize/i.test(chrome)) throw new Error("LB-015 custom titlebar exposes forbidden maximize behavior");

console.log("LB015_BUTTON_PROMPT_CONTRACT=PASS coherent_geometry=true white_surface_affordance=true states=true minimal_prompt=true safety_copy_preserved=true single_custom_chrome=true fixed_content_area=true drag=true minimize=true close=true maximize=false");
