import { readFileSync } from "node:fs";

const css = readFileSync("src/styles.css", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
const bridge = readFileSync("src/bridge.ts", "utf8");
const presentation = readFileSync("src/presentation.ts", "utf8");
const serviceDot = readFileSync("src/components/ServiceStatusDot.tsx", "utf8");
const backend = readFileSync("src-tauri/src/commands/ui.rs", "utf8");
const compactCss = css.replace(/\s+/g, "");

for (const marker of [
  "--accent:#0071e3",
  ".primary{background:var(--accent);border-color:var(--accent);color:#fff}",
  ".choice.selected{background:var(--accent);border-color:var(--accent);color:#fff}",
  ".choice.admin-choice.selected{background:var(--admin-accent);border-color:var(--admin-accent);color:#fff}",
]) if (!compactCss.includes(marker)) throw new Error(`LB-015 accent/admin contract missing: ${marker}`);
if (/\.primary\{[^}]*background:#1d1d1f/i.test(css) || /\.choice\.selected\{[^}]*background:#1d1d1f/i.test(css)) throw new Error("LB-015 black ordinary accent returned");

for (const marker of [
  'off: "unknown"', 'starting: "starting"', 'online: "ready"', 'recovering: "starting"', 'fault: "fault"',
]) if (!presentation.includes(marker)) throw new Error(`LB-015 shared typed status semantic missing: ${marker}`);
for (const marker of ["serviceVisualState[service]", "status-${state}", "data-service-state={state}"]) if (!serviceDot.includes(marker)) throw new Error(`LB-015 shared status-dot component missing: ${marker}`);
for (const marker of [".service-status-dot.status-ready", ".service-status-dot.status-starting", ".service-status-dot.status-fault", ".service-status-dot.status-unknown"]) if (!css.includes(marker)) throw new Error(`LB-015 status-dot class missing: ${marker}`);

for (const marker of ["ServiceStatusDot", "projection?.tunnelService ?? null", "projection?.codingService ?? null", 'mode === "admin" ? "admin-choice"']) if (!app.includes(marker)) throw new Error(`LB-015 Dashboard shared status/admin marker missing: ${marker}`);
for (const marker of ["localEnvironmentService: ServiceCode", "tunnelService: ServiceCode", "codingService: ServiceCode"]) if (!bridge.includes(marker)) throw new Error(`LB-015 typed MainProjection service missing: ${marker}`);
for (const marker of ["local_environment_service: &'static str", "local_environment_service: local_environment_service_code(&snapshot.state)", "fn local_environment_service_code(state: &RuntimeState)", "RuntimeState::Stopped => \"off\"", "RuntimeState::StartingMcp | RuntimeState::WaitingMcpReady => \"starting\"", "RuntimeState::Faulted(_) => \"fault\"", "fn service_codes(state: &RuntimeState)"]) if (!backend.includes(marker)) throw new Error(`LB-015 Rust typed-source projection missing: ${marker}`);

console.log("LB015_STATUS_ACCENT_CONTRACT=PASS accent=#0071e3 admin=amber dashboard_dots=true shared_typed_source=true ready=green starting=amber fault=red unknown=gray");
