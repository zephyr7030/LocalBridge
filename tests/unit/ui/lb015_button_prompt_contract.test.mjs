import { readFileSync } from "node:fs";

const css = readFileSync("src/styles.css", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
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
if (!compactCss.includes("html,body,#root{width:100%;min-width:0;min-height:100%}")) {
  throw new Error("LB-015 document/root surface is not bound to the WebView viewport");
}
if (!compactCss.includes("body{margin:0;min-width:320px;min-height:100dvh}")
  || !compactCss.includes("#root{min-height:100dvh;background:#f5f5f7}")) {
  throw new Error("LB-015 body/root does not fill the viewport background surface");
}
const shellRule = compactCss.match(/\.shell\{([^}]*)\}/)?.[1] ?? "";
if (!shellRule.includes("width:calc(100%-clamp(32px,6vw,72px))") || !shellRule.includes("max-width:1180px")) {
  throw new Error("LB-015 Dashboard shell is not viewport-responsive on wide windows");
}
if (/\.shell\{[^}]*760px/.test(compactCss)) {
  throw new Error("LB-015 Dashboard regressed to the obsolete 760px narrow-column cap");
}

console.log("LB015_BUTTON_PROMPT_CONTRACT=PASS coherent_geometry=true white_surface_affordance=true states=true minimal_prompt=true safety_copy_preserved=true viewport_surface=true responsive_dashboard=true");
