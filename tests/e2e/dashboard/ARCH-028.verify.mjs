import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(process.env.LOCALBRIDGE_REPO_ROOT || ".");
const ruleId = process.env.LOCALBRIDGE_ARCH_RULE_ID;
if (ruleId && ruleId !== "ARCH-028") throw new Error(`ARCH-028 invoked as ${ruleId}`);

const contracts = JSON.parse(readFileSync(join(root, "PR_CONTRACTS.json"), "utf8"));
for (const [key, expected] of Object.entries({
  ui_text_button_content_sized_required: true,
  ui_text_button_arbitrary_fixed_geometry_forbidden: true,
  ui_permission_mode_three_way_group_content_size_exempt: true,
  ui_permission_mode_three_way_group_layout: "symmetric_equal_three_columns",
  ui_permission_mode_three_way_group_equal_width_required: true,
  ui_permission_mode_three_way_group_equal_height_required: true,
  ui_permission_mode_three_way_group_text_clipping_forbidden: true,
})) if (contracts.rules?.[key] !== expected) throw new Error(`ARCH-028 contract drift: ${key}`);
if (JSON.stringify(contracts.rules.ui_permission_mode_three_way_group_labels) !== JSON.stringify(["编辑模式", "完整模式", "管理员模式"])) throw new Error("ARCH-028 PermissionMode labels drifted");
if (JSON.stringify(contracts.rules.ui_typography_tiers) !== JSON.stringify(["title", "body", "auxiliary"])) throw new Error("ARCH-028 typography tier inventory drifted");

const cssFiles = [];
const walk = (dir) => {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    const stat = statSync(path);
    if (stat.isDirectory()) walk(path);
    else if (entry.endsWith(".css")) cssFiles.push(path);
  }
};
walk(join(root, "src"));
const allowedFontSizes = new Set(["var(--font-title)", "var(--font-body)", "var(--font-auxiliary)"]);
const observedFontSizes = new Set();
for (const file of cssFiles) {
  const css = readFileSync(file, "utf8");
  for (const match of css.matchAll(/font-size\s*:\s*([^;}]+)/g)) {
    const value = match[1].trim();
    observedFontSizes.add(value);
    if (!allowedFontSizes.has(value)) throw new Error(`ARCH-028 fourth/non-token font-size in ${file}: ${value}`);
  }
}
for (const value of allowedFontSizes) if (!observedFontSizes.has(value)) throw new Error(`ARCH-028 unused typography tier: ${value}`);

const mainCss = readFileSync(join(root, "src", "styles.css"), "utf8").replace(/\s+/g, "");
const onboardingCss = readFileSync(join(root, "src", "features", "onboarding", "onboarding.css"), "utf8").replace(/\s+/g, "");
const onboardingTsx = readFileSync(join(root, "src", "features", "onboarding", "Onboarding.tsx"), "utf8");
const genericButtons = mainCss.match(/\.ghost,\.secondary,\.primary,\.choice\{([^}]*)\}/)?.[1] ?? "";
for (const marker of ["width:max-content", "max-width:100%", "height:auto", "font-size:var(--font-body)", "white-space:normal"]) if (!genericButtons.includes(marker)) throw new Error(`ARCH-028 generic text-button content geometry missing: ${marker}`);
if (genericButtons.includes("min-height:")) throw new Error("ARCH-028 generic text buttons retain fixed/minimum height");
if (!mainCss.includes(".settings-summary{display:grid;grid-template-columns:minmax(0,1fr)max-contentmax-content")) throw new Error("ARCH-028 Settings action columns are not content-derived max-content columns");
for (const forbidden of [
  ".settings-summary>.settings-clear{grid-column:2;width:",
  ".settings-summary>.settings-replace{grid-column:3;width:",
  ".settings-summary>.settings-delete-cancel{grid-column:2;width:",
  ".settings-summary>.settings-confirm-delete{grid-column:3;width:",
  ".onboarding-copy-action{width:",
  ".onboarding-copy-action{min-width:",
  "min-height:96px",
]) if (`${mainCss}\n${onboardingCss}`.includes(forbidden)) throw new Error(`ARCH-028 arbitrary fixed text-button geometry remains: ${forbidden}`);

if (!mainCss.includes(".access-grid{display:grid;grid-template-columns:repeat(3,1fr)") || !mainCss.includes(".access-grid>.choice{width:100%;height:100%}")) throw new Error("ARCH-028 Settings PermissionMode group is not symmetric equal-three-cell geometry");
if (!onboardingCss.includes(".onboarding-permissions{display:grid;grid-template-columns:repeat(3,minmax(0,1fr))") || !onboardingCss.includes(".choice.onboarding-permission{width:100%;height:100%")) throw new Error("ARCH-028 onboarding PermissionMode group is not symmetric equal-three-cell geometry");
if (onboardingCss.includes(".onboarding-permissions{grid-template-columns:1fr}")) throw new Error("ARCH-028 onboarding PermissionMode group has a one-column override");
for (const marker of ["Math.max(...widths) - Math.min(...widths) <= 0.5", "Math.max(...heights) - Math.min(...heights) <= 0.5", "Math.abs(gaps[0] - gaps[1]) <= 0.5", "lineBoxesInside(button, button.querySelector(\"strong\"))", "lineBoxesInside(button, button.querySelector(\"small\"))"]) if (!onboardingTsx.includes(marker)) throw new Error(`ARCH-028 rendered equal-three-cell geometry gate missing: ${marker}`);

console.log("ARCH-028_VERIFY=PASS typography=title,body,auxiliary generic_text_buttons=content-derived permission_group=equal-three-cell-exception clipping=guarded");
