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
  'payload_tree_sha256 = "7dd8817d8bb795608f378ee085e5436ddd2a1854c719ea63432c8e7c0c50374b"',
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
if (treeSha("runtime/coding-tools-mcp") !== "7dd8817d8bb795608f378ee085e5436ddd2a1854c719ea63432c8e7c0c50374b") throw new Error("coding-tools payload tree SHA256 mismatch");

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

console.log("RUNTIME_MANIFEST_VERIFY=PASS python=3.12.10 coding_tools=0.2.2 pyjwt=2.10.1 exact_hashes=true runtime_pip=false external_python_fallback=false");
