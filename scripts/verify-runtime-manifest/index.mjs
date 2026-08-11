import { readFileSync } from "node:fs";
const text = readFileSync("runtime-manifest.toml", "utf8");
const required = [
  'target = "windows-x86_64"',
  'target_os = "windows"',
  'minimum_os = "windows-11"',
  'arch = "x86_64"',
  'bundle_webview2 = false',
  'webview2_source = "windows-system-runtime"',
  'runtime_self_update = false',
  'application_auto_update = false',
  'telemetry = false',
];
for (const item of required) if (!text.includes(item)) throw new Error(`runtime manifest requirement missing: ${item}`);
console.log("RUNTIME_MANIFEST_VERIFY=PASS");
