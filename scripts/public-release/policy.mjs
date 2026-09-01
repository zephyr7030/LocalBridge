const slash = (value) => String(value ?? "").replaceAll("\\", "/").replace(/^\.\//, "");

export const PRIVATE_EXACT = new Set([
  "AGENTS.md",
  "START_HERE.md",
  "PR_INDEX.json",
  "PR_CONTRACTS.json",
  "PROJECT_STATE.json",
  "FINAL_REVIEW.json",
  "ARCHITECTURE_RULES.json",
  "PACKAGE_INVENTORY.json",
  "COMPATIBILITY_BASELINE.json",
]);

export const PRIVATE_PREFIXES = [
  ".git/",
  ".coding-tools/",
  "governance/",
  "skills/",
  "templates/",
  "compatibility/",
  "spikes/",
  "release-artifacts/",
  "scripts/authorization-records/",
  "scripts/verify-architecture/",
  "tests/fixtures/architecture/",
  "tests/integration/upstream_spike/",
];

const PUBLIC_EXACT = new Set([
  ".gitignore",
  ".gitattributes",
  "LICENSE",
  "README.md",
  "SECURITY.md",
  "CONTRIBUTING.md",
  "CHANGELOG.md",
  "THIRD_PARTY_NOTICES.md",
  "package.json",
  "package-lock.json",
  "tsconfig.json",
  "tsconfig.app.json",
  "tsconfig.node.json",
  "vite.config.ts",
  "start-localbridge.cmd",
  "runtime-manifest.toml",
  "runtime-policy.toml",
  "scripts/prepare-toolbox.mjs",
  "scripts/prepare-lb018-resources.mjs",
  "compatibility/coding-tools/0.2.2/tools-list.json",
]);

const PUBLIC_PREFIXES = [
  ".github/",
  "assets/",
  "schema/",
  "src/",
  "src-tauri/",
  "runtime/",
  "scripts/public-release/",
  "scripts/test/",
  "scripts/verify-schema44/",
  "tests/",
];

export function normalizeRepoPath(value) {
  return slash(value);
}

export function isPrivatePath(value) {
  const path = slash(value);
  if (!path || path === ".git" || path === ".coding-tools") return true;
  if (PUBLIC_EXACT.has(path)) return false;
  if (PRIVATE_EXACT.has(path)) return true;
  if (PRIVATE_PREFIXES.some((prefix) => path.startsWith(prefix))) return true;
  if (/^授权信息(?:\.|$)/i.test(path)) return true;
  if (/authorization/i.test(path) && !path.startsWith("src-tauri/")) return true;
  if (/cloudflared(?:\.exe|-manifest\.json)$/i.test(path)) return true;
  if (
    path.startsWith("tests/")
    && path.endsWith(".mjs")
    && !path.startsWith("tests/integration/release-preflight/")
    && !path.startsWith("tests/black-box/chatgpt/")
  ) return true;
  return false;
}

export function isPublicPath(value) {
  const path = slash(value);
  if (isPrivatePath(path)) return false;
  return PUBLIC_EXACT.has(path) || PUBLIC_PREFIXES.some((prefix) => path.startsWith(prefix));
}

export function sanitizePublicText(value, text) {
  const path = slash(value);
  if (path === ".gitattributes") {
    const pinnedRuntimeRules = [
      "runtime/python/** -text -eol",
      "runtime/coding-tools-mcp/** -text -eol",
      "runtime/tunnel-client/** -text -eol",
    ];
    const lines = String(text).split(/\r?\n/);
    const missing = pinnedRuntimeRules.filter((rule) => !lines.includes(rule));
    if (missing.length === 0) return text;
    return `${String(text).trimEnd()}\n\n# Byte-pinned bundled runtimes must survive Git add/checkout unchanged.\n${missing.join("\n")}\n`;
  }
  if (path === "package.json") {
    const manifest = JSON.parse(text);
    const publicScripts = new Set(["dev", "build", "toolbox:prepare", "test"]);
    manifest.scripts = Object.fromEntries(Object.entries(manifest.scripts ?? {}).filter(([name]) => publicScripts.has(name)));
    manifest.license = manifest.license ?? "MIT";
    return `${JSON.stringify(manifest, null, 2)}\n`;
  }
  if (path === "runtime-manifest.toml") {
    return text.replace(/(?:^|\r?\n)\[verification\]\r?\n[\s\S]*?(?=\r?\n\[privileged_broker\])/m, "\n");
  }
  return text;
}

export const PUBLIC_FORBIDDEN_TEXT = [
  "PR_CONTRACTS.json",
  "PROJECT_STATE.json",
  "PR_INDEX.json",
  "FINAL_REVIEW.json",
  "governance/G4_",
];
