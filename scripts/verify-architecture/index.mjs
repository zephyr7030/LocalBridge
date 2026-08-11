import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { resolve, relative, join } from "node:path";

const args = process.argv.slice(2);
const rootArg = args[args.indexOf("--root") + 1];
const root = resolve(rootArg && !rootArg.startsWith("--") ? rootArg : ".");
const expectFailure = args.includes("--expect-failure");
const repoRoot = resolve(".");
const rules = JSON.parse(readFileSync(join(repoRoot, "ARCHITECTURE_RULES.json"), "utf8"));
if (!Array.isArray(rules.rules) || rules.rules.length === 0) throw new Error("ARCHITECTURE_RULES.json has no rules");

const ignoredSegments = new Set(["node_modules", ".git", ".coding-tools", "target", "artifacts"]);

function filesUnder(dir) {
  if (!existsSync(dir)) return [];
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const s = statSync(p);
    if (s.isDirectory()) {
      if (ignoredSegments.has(name)) continue;
      if (resolve(p) === resolve(repoRoot, "scripts", "verify-architecture")) continue;
      out.push(...filesUnder(p));
    }
    else if (/\.(ts|tsx|js|mjs|rs|json|toml)$/.test(name)) out.push(p);
  }
  return out;
}

const findings = [];
const all = root === repoRoot
  ? [
      ...filesUnder(join(root, "src")),
      ...filesUnder(join(root, "src-tauri")),
      ...filesUnder(join(root, "scripts")),
    ]
  : filesUnder(root);
const text = (p) => readFileSync(p, "utf8");
const rel = (p) => relative(root, p).replaceAll("\\", "/");
const ruleIds = new Set(rules.rules.map((r) => r.id));
for (const id of ["ARCH-001", "ARCH-002", "ARCH-003", "ARCH-023", "ARCH-024"]) {
  if (!ruleIds.has(id)) throw new Error(`required architecture rule missing: ${id}`);
}
for (const p of all) {
  const r = rel(p);
  const body = text(p);
  if (r.startsWith("src/") && /@tauri-apps\/plugin-shell|child_process|Command\.create|new\s+Command\s*\(/.test(body)) findings.push(["ARCH-001", r]);
  if ((r.startsWith("src-tauri/") || r.startsWith("scripts/")) && /python from PATH|py\.exe fallback|user site-packages/i.test(body)) findings.push(["ARCH-002", r]);
  const literals = [...body.matchAll(/["'`]([^"'`\r\n]*)["'`]/g)].map((m) => m[1].trim());
  const forbiddenBind = literals.some((value) =>
    value === "0.0.0.0" ||
    value === "::" ||
    /^0\.0\.0\.0:\d+$/.test(value) ||
    /^\[::\]:\d+$/.test(value),
  );
  if (forbiddenBind) findings.push(["ARCH-003", r]);
}
if (root === repoRoot) {
  const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
  const deps = { ...(pkg.dependencies ?? {}), ...(pkg.devDependencies ?? {}) };
  const forbiddenDependency = Object.keys(deps).find((n) => /mui|chakra|antd|bootstrap|tailwind|framer-motion|lucide|heroicons|fontawesome/i.test(n));
  if (forbiddenDependency) findings.push(["ARCH-023", `package.json:${forbiddenDependency}`]);
  const pr = JSON.parse(readFileSync(join(root, "PR_INDEX.json"), "utf8"));
  const g0 = pr.groups.find((g) => g.id === "G0");
  const g1 = pr.groups.find((g) => g.id === "G1");
  if (g1?.status !== "BLOCKED" && g0?.review_status !== "PASS") findings.push(["ARCH-024", "PR_INDEX.json"]);
}

if (expectFailure) {
  if (findings.length === 0) throw new Error("intentional architecture violation was not detected");
  console.log(`ARCHITECTURE_NEGATIVE_FIXTURE=PASS findings=${findings.length}`);
  process.exit(0);
}
if (findings.length) {
  for (const [id, file] of findings) console.error(`${id}: ${file}`);
  process.exit(1);
}
console.log(`ARCHITECTURE_VERIFY=PASS rules=${rules.rules.length}`);
