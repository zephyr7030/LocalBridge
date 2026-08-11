import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
function walk(dir) {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name); const s = statSync(p);
    return s.isDirectory() ? walk(p) : /\.(tsx|ts|html)$/.test(name) ? [p] : [];
  });
}
const forbidden = ["Dashboard", "Settings", "Diagnostics", "Elevated", "Broker", "Runtime"];
const findings = [];
for (const file of walk("src")) {
  if (file.includes("__tests__")) continue;
  const body = readFileSync(file, "utf8");
  for (const word of forbidden) if (new RegExp(`>[\\s]*${word}[\\s]*<|[\"']${word}[\"']`).test(body)) findings.push(`${file}:${word}`);
}
if (findings.length) throw new Error(`unnecessary English UI: ${findings.join(", ")}`);
console.log("UI_LANGUAGE_VERIFY=PASS");
