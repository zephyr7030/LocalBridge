import { execFileSync, spawn } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";

const ROOT = process.cwd();
const TARGET_DIR = join(ROOT, "src-tauri", "target-fixed-window-e2e");
const PRODUCTION_ASSETS = process.argv.includes("--production-assets");
if (process.platform !== "win32") throw new Error("LB-016 fixed-window E2E requires Windows/Tauri/WebView2");

async function freePort() {
  return await new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : null;
      server.close((error) => error ? reject(error) : resolve(port));
    });
  });
}

function stopTree(child) {
  if (!child?.pid) return;
  try { execFileSync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" }); } catch {}
}

async function runView(view) {
  const devPort = await freePort();
  const configPath = join("tests", "artifacts", `fixed-window-e2e-${process.pid}-${view}.json`);
  mkdirSync(join(ROOT, "tests", "artifacts"), { recursive: true });
  writeFileSync(join(ROOT, configPath), JSON.stringify({
    identifier: `com.localbridge.desktop.fixedwindowe2e.${view}`,
    build: PRODUCTION_ASSETS ? { beforeBuildCommand: "npm run build" } : {
      devUrl: `http://127.0.0.1:${devPort}${view === "onboarding" ? "?lb016-e2e=permission-geometry" : ""}`,
      beforeDevCommand: `npm run dev -- --port ${devPort}`,
    },
    bundle: { active: false },
  }, null, 2));
  if (PRODUCTION_ASSETS) {
    execFileSync(process.execPath, [
      "node_modules/@tauri-apps/cli/tauri.js", "build", "--debug", "--no-bundle", "--config", configPath,
    ], {
      cwd: ROOT,
      env: { ...process.env, CARGO_TARGET_DIR: TARGET_DIR },
      windowsHide: true,
      stdio: "inherit",
      timeout: 600_000,
    });
  }
  return await new Promise((resolve, reject) => {
    let output = "";
    const program = PRODUCTION_ASSETS ? join(TARGET_DIR, "debug", "localbridge.exe") : "cmd.exe";
    const args = PRODUCTION_ASSETS ? [] : ["/d", "/s", "/c", `node_modules\\.bin\\tauri.cmd dev --no-watch --config ${configPath}`];
    const child = spawn(program, args, {
      cwd: ROOT,
      env: {
        ...process.env,
        LOCALBRIDGE_FIXED_WINDOW_E2E_VIEW: view,
        ...(PRODUCTION_ASSETS ? { LOCALBRIDGE_CSP_E2E: "1" } : {}),
        // Only the dev run can deep-link to Screen 3, so only that run asks the
        // harness to assert the equal-thirds permission geometry.
        ...(!PRODUCTION_ASSETS && view === "onboarding"
          ? { LOCALBRIDGE_PERMISSION_GEOMETRY_E2E: "1" }
          : {}),
        CARGO_TARGET_DIR: TARGET_DIR,
      },
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const timer = setTimeout(() => {
      stopTree(child);
      reject(new Error(`Timed out waiting for ${view} fixed-window E2E.\n${output}`));
    }, 180_000);
    const consume = (chunk) => {
      output = (output + chunk).slice(-60_000);
      if (output.includes(`LB016_FIXED_WINDOW_E2E=FAIL view=${view}`)) {
        clearTimeout(timer);
        stopTree(child);
        reject(new Error(output));
      }
    };
    child.stdout.on("data", consume);
    child.stderr.on("data", consume);
    child.once("exit", (code) => {
      clearTimeout(timer);
      rmSync(join(ROOT, configPath), { force: true });
      const marker = output.split(/\r?\n/).find((line) => line.includes(`LB016_FIXED_WINDOW_E2E=PASS view=${view}`))?.trim();
      if (code !== 0 || !marker) reject(new Error(`Fixed-window E2E exited ${code} for ${view}.\n${output}`));
      else resolve(marker);
    });
  });
}

const requestedView = process.argv.slice(2).find((argument) => !argument.startsWith("--"));
if (requestedView) {
  if (!['onboarding', 'dashboard'].includes(requestedView)) throw new Error(`Unknown fixed-window E2E view: ${requestedView}`);
  const marker = await runView(requestedView);
  console.log(`LB016_FIXED_WINDOW_E2E=PASS view=${requestedView} logical_fixed=780x620 native_dpi_scaling=true native_decorations=false resizable=false maximizable=false single_custom_chrome=true edge_to_edge=true controls=drag,minimize,close maximize=false`);
  console.log(marker);
} else {
  const onboarding = await runView("onboarding");
  const dashboard = await runView("dashboard");
  console.log("LB016_FIXED_WINDOW_E2E=PASS views=2 logical_fixed=780x620 native_dpi_scaling=true native_decorations=false resizable=false maximizable=false single_custom_chrome=true edge_to_edge=true controls=drag,minimize,close maximize=false");
  console.log(onboarding);
  console.log(dashboard);
}
