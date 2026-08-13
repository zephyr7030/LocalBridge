import { execFileSync, spawn } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";

const ROOT = process.cwd();
const TARGET_DIR = join(ROOT, "src-tauri", "target-resize-e2e");

if (process.platform !== "win32") throw new Error("LB-016 real resize E2E requires Windows/Tauri/WebView2");

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
  const configPath = join("tests", "artifacts", `resize-e2e-${process.pid}-${view}.json`);
  mkdirSync(join(ROOT, "tests", "artifacts"), { recursive: true });
  writeFileSync(join(ROOT, configPath), JSON.stringify({
    identifier: `com.localbridge.desktop.resizee2e.${view}`,
    build: {
      devUrl: `http://127.0.0.1:${devPort}`,
      beforeDevCommand: `npm run dev -- --port ${devPort}`,
    },
    bundle: { active: false },
  }, null, 2));

  let output = "";
  const child = spawn("cmd.exe", ["/d", "/s", "/c", `node_modules\\.bin\\tauri.cmd dev --no-watch --config ${configPath}`], {
    cwd: ROOT,
    env: { ...process.env, LOCALBRIDGE_RESIZE_E2E_VIEW: view, CARGO_TARGET_DIR: TARGET_DIR },
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });

  try {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`Timed out waiting for ${view} resize E2E.\n${output}`)), 180_000);
      const consume = (chunk) => {
        output = (output + chunk).slice(-40_000);
        if (output.includes(`LB016_REAL_RESIZE_E2E=FAIL view=${view}`)) {
          clearTimeout(timer);
          reject(new Error(output));
        } else if (output.includes(`LB016_REAL_RESIZE_E2E=PASS view=${view}`)) {
          clearTimeout(timer);
          resolve();
        }
      };
      child.stdout.on("data", consume);
      child.stderr.on("data", consume);
      child.once("exit", (code) => {
        if (!output.includes(`LB016_REAL_RESIZE_E2E=PASS view=${view}`)) {
          clearTimeout(timer);
          reject(new Error(`Tauri exited ${code} before ${view} resize E2E passed.\n${output}`));
        }
      });
    });
    return output.split(/\r?\n/).find((line) => line.includes(`LB016_REAL_RESIZE_E2E=PASS view=${view}`))?.trim();
  } finally {
    stopTree(child);
    rmSync(join(ROOT, configPath), { force: true });
  }
}

const onboarding = await runView("onboarding");
const dashboard = await runView("dashboard");
if (!onboarding || !dashboard) throw new Error("LB-016 real resize E2E PASS marker missing");
console.log("LB016_REAL_RESIZE_E2E=PASS views=2 native_webview_sync=true maximize=true");
console.log(onboarding);
console.log(dashboard);
