import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const sha = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const treeSha = (root, excludedName = "runtime-metadata.json") => {
  const base = resolve(root);
  const entries = [];
  const visit = (dir) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      const stat = statSync(path);
      if (stat.isDirectory()) visit(path);
      else if (stat.isFile() && name !== excludedName) {
        entries.push(`${relative(base, path).replaceAll("\\", "/")}\0${sha(path)}`);
      }
    }
  };
  visit(base);
  entries.sort();
  return createHash("sha256").update(entries.join("\n"), "utf8").digest("hex");
};

const text = readFileSync("runtime-manifest.toml", "utf8");
const toolboxPrepare = readFileSync("scripts/prepare-toolbox.mjs", "utf8");
const releasePrepare = readFileSync("scripts/prepare-lb018-resources.mjs", "utf8");
const thirdPartyNotices = readFileSync("THIRD_PARTY_NOTICES.md", "utf8");
const packageJson = JSON.parse(readFileSync("package.json", "utf8"));
const tauri = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
if (/cloudflared|cloudflare managed/i.test(text)) throw new Error("final runtime manifest must not contain Cloudflare runtime entries");
if (existsSync("runtime/tunnel-client/cloudflared.exe") || existsSync("runtime/tunnel-client/cloudflared-manifest.json")) throw new Error("final runtime payload must not contain cloudflared");
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

const exact = [
  'schema_version = 3',
  'version = "0.2.2"',
  'git_commit = "311c1f2529d0f047ad2a8b68db6bf92dbb93d6bc"',
  'runtime_subset_sha256 = "cc2171854ce0035942b752ce88bb3aec2e286cdf9603dd51ad41734ea70dcda6"',
  'payload_tree_sha256 = "5039d4c696636c42e3f965116a0cfcc9bcd27a7ea782a013a04dff4e09945821"',
  'dependency_pyjwt_version = "2.10.1"',
  'dependency_pyjwt_wheel_sha256 = "dcdd193e30abefd5debf142f9adfcdd2b58004e644f25406ffaebd50bd98dacb"',
  'auth_secret_injection = "child_environment:CODING_TOOLS_MCP_AUTH_TOKEN"',
  'bind_host = "127.0.0.1"',
  'telemetry_forced_off = true',
  'runtime_pip_install = false',
  'stable_adapter = "src-tauri/src/mcp"',
  'version = "3.12.10"',
  'archive = "python-3.12.10-embed-amd64.zip"',
  'executable = "runtime/python/python.exe"',
  'executable_sha256 = "4d6f5f81a4bca11191c4c7c6b43632694d0a4ce74e068619d8fdc161d469859a"',
  'python312_dll_sha256 = "9a0e3435aaa680d868150f87ab3e388ad2eebc22f87e036155c7b4eda8cd2120"',
  'stdlib_zip_sha256 = "fb131c0ef7e35cc5250a74c8cd18744bf4115fb8163710711f3758d7df3d1f88"',
  'pth_sha256 = "3840e706682aa41ec7e599a50763bec6c6ddd6bde66e81c64afe2394539ea4fa"',
  'isolated = true',
  'user_site_enabled = false',
  'runtime_pip_present = false',
  'external_python_fallback = false',
];
for (const item of exact) if (!text.includes(item)) throw new Error(`LB-006 exact runtime manifest requirement missing: ${item}`);
if (text.includes("TO_BE_FILLED_BY_LB_006")) throw new Error("LB-006 runtime manifest still contains placeholder");

const toolboxExact = [
  'runtime_root = "runtime/toolbox"',
  'build_prepare_script = "scripts/prepare-toolbox.mjs"',
  'runtime_download = false',
  'runtime_update = false',
  'persistent_path_mutation = false',
  'public_tools = false',
  'logical_name = "aria2c"',
  'version = "1.37.0"',
  'source = "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip"',
  'archive_sha256 = "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288"',
  'executable = "runtime/toolbox/bin/aria2c.exe"',
  'executable_sha256 = "be2099c214f63a3cb4954b09a0becd6e2e34660b886d4c898d260febfe9d70c2"',
  'logical_name = "7z"',
  'version = "26.02"',
  'source = "https://www.7-zip.org/a/7z2602-extra.7z"',
  'archive_sha256 = "081df9e9311dfd9c9e0e98c1c80180b99bb51e4cb24156b5f3057fe3c259d70a"',
  'source_member = "x64/7za.exe"',
  'executable = "runtime/toolbox/bin/7z.exe"',
  'executable_sha256 = "35d4d69d7cd6cb44558f208c3b1334268013f9daf82d2dda848893a1c30c59c2"',
  'logical_name = "jq"',
  'version = "1.8.2"',
  'source = "https://github.com/jqlang/jq/releases/download/jq-1.8.2/jq-windows-amd64.exe"',
  'archive_sha256 = "a6fc67fedaf9128a3309a1e2ebb8b986aeccf70122ee46d2cb4849e423f0c627"',
  'executable = "runtime/toolbox/bin/jq.exe"',
  'executable_sha256 = "a6fc67fedaf9128a3309a1e2ebb8b986aeccf70122ee46d2cb4849e423f0c627"',
  'source = "windows-system-runtime"',
  'executable = "%SystemRoot%/System32/curl.exe"',
  'startup_existence_probe = true',
  'startup_capability_probe = true',
  'missing_error = "RuntimeUnavailable"',
  'capability_missing_error = "CapabilityUnavailable"',
  'fallback_download = false',
  'bundle_toolbox = true',
];
for (const item of toolboxExact) if (!text.includes(item)) throw new Error(`schema42 toolbox manifest requirement missing: ${item}`);
if (packageJson.scripts?.["toolbox:prepare"] !== "node scripts/prepare-toolbox.mjs") throw new Error("schema42 toolbox build preparation script missing");
if (tauri.build?.beforeDevCommand !== "npm run dev" || !tauri.build?.beforeBuildCommand?.includes("prepare-lb018-resources.mjs") || !releasePrepare.includes("scripts/prepare-toolbox.mjs")) throw new Error("schema42 toolbox acquisition is not build-only");
if (tauri.bundle?.resources?.["target/toolbox-stage/"] !== "runtime/toolbox/") throw new Error("schema42 toolbox release resource mapping missing");
for (const marker of [
  "release-1.37.0/aria2-1.37.0-win-64bit-build1.zip",
  "7z2602-extra.7z",
  "jq-1.8.2/jq-windows-amd64.exe",
  "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288",
  "081df9e9311dfd9c9e0e98c1c80180b99bb51e4cb24156b5f3057fe3c259d70a",
  "a6fc67fedaf9128a3309a1e2ebb8b986aeccf70122ee46d2cb4849e423f0c627",
]) if (!toolboxPrepare.includes(marker) || !thirdPartyNotices.includes(marker)) throw new Error(`schema42 toolbox provenance missing: ${marker}`);
const systemRootMarker = String.fromCharCode(37) + "SystemRoot" + String.fromCharCode(37);
if (![systemRootMarker,"System32","curl.exe"].every((marker) => toolboxPrepare.includes(marker)) || toolboxPrepare.includes("setx") || toolboxPrepare.includes("process.env.PATH =")) throw new Error("schema42 toolbox PATH/curl build contract drifted");
const toolboxStage = "src-tauri/target/toolbox-stage/bin";
if (existsSync(toolboxStage)) {
  for (const [name, expected] of [
    ["aria2c.exe", "be2099c214f63a3cb4954b09a0becd6e2e34660b886d4c898d260febfe9d70c2"],
    ["7z.exe", "35d4d69d7cd6cb44558f208c3b1334268013f9daf82d2dda848893a1c30c59c2"],
    ["jq.exe", "a6fc67fedaf9128a3309a1e2ebb8b986aeccf70122ee46d2cb4849e423f0c627"],
  ]) if (sha(join(toolboxStage, name)) !== expected) throw new Error(`schema42 toolbox staged executable SHA256 mismatch: ${name}`);
}

const pythonMeta = JSON.parse(readFileSync("runtime/python/runtime-metadata.json", "utf8"));
const codingMeta = JSON.parse(readFileSync("runtime/coding-tools-mcp/runtime-metadata.json", "utf8"));
if (pythonMeta.version !== "3.12.10" || pythonMeta.external_python_fallback !== false || pythonMeta.runtime_pip_present !== false || pythonMeta.isolated !== true || pythonMeta.user_site_enabled !== false) throw new Error("Python runtime metadata contract mismatch");
if (codingMeta.version !== "0.2.2" || codingMeta.git_commit !== "311c1f2529d0f047ad2a8b68db6bf92dbb93d6bc" || codingMeta.runtime_pip_present !== false || codingMeta.dependency_pyjwt_version !== "2.10.1") throw new Error("coding-tools runtime metadata contract mismatch");

const critical = [
  ["runtime/python/python.exe", "4d6f5f81a4bca11191c4c7c6b43632694d0a4ce74e068619d8fdc161d469859a"],
  ["runtime/python/python312.dll", "9a0e3435aaa680d868150f87ab3e388ad2eebc22f87e036155c7b4eda8cd2120"],
  ["runtime/python/python312.zip", "fb131c0ef7e35cc5250a74c8cd18744bf4115fb8163710711f3758d7df3d1f88"],
  ["runtime/python/python312._pth", "3840e706682aa41ec7e599a50763bec6c6ddd6bde66e81c64afe2394539ea4fa"],
];
for (const [path, expected] of critical) {
  if (!existsSync(path)) throw new Error(`bundled runtime file missing: ${path}`);
  if (sha(path) !== expected) throw new Error(`bundled runtime SHA256 mismatch: ${path}`);
}
if (treeSha("runtime/python") !== "48546587a8bb59d03016ea4edf82c292a477dec6acec530745b78c8935558682") throw new Error("Python payload tree SHA256 mismatch");
const codingToolsTreeSha = treeSha("runtime/coding-tools-mcp");
if (codingToolsTreeSha !== "5039d4c696636c42e3f965116a0cfcc9bcd27a7ea782a013a04dff4e09945821") throw new Error(`coding-tools payload tree SHA256 mismatch: ${codingToolsTreeSha}`);

const installerName = /^(?:pip|pip-[0-9].*\.dist-info|setuptools|setuptools-[0-9].*\.dist-info|wheel|wheel-[0-9].*\.dist-info)$/i;
const scanInstallerDirs = (root) => {
  for (const name of readdirSync(root)) {
    const path = join(root, name);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      if (installerName.test(name)) throw new Error(`runtime installer payload forbidden: ${path}`);
      scanInstallerDirs(path);
    }
  }
};
scanInstallerDirs("runtime/python");
scanInstallerDirs("runtime/coding-tools-mcp");

console.log("RUNTIME_MANIFEST_VERIFY=PASS python=3.12.10 coding_tools=0.2.2 toolbox=aria2c-1.37.0,7z-26.02,jq-1.8.2 system32_curl=true exact_hashes=true runtime_download=false");
