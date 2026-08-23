import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const run = (program, args) => {
  const result = spawnSync(program, args, { cwd: root, stdio: "inherit", windowsHide: true });
  if (result.status !== 0) throw new Error(`${program} ${args.join(" ")} failed (${result.status ?? "start failure"})`);
};

for (const forbidden of ["runtime/tunnel-client/cloudflared.exe", "runtime/tunnel-client/cloudflared-manifest.json"]) {
  if (existsSync(resolve(root, forbidden))) throw new Error(`forbidden final runtime payload exists: ${forbidden}`);
}
for (const required of [
  "runtime/python/python.exe",
  "runtime/coding-tools-mcp/coding_tools_mcp/__init__.py",
  "runtime/tunnel-client/tunnel-client.exe",
  "runtime-manifest.toml",
  "runtime-policy.toml",
  "LICENSE",
  "THIRD_PARTY_NOTICES.md",
]) if (!existsSync(resolve(root, required))) throw new Error(`required release payload missing: ${required}`);

const manifest = readFileSync(resolve(root, "runtime-manifest.toml"), "utf8");
if (/cloudflared|cloudflare managed/i.test(manifest)) throw new Error("runtime-manifest still advertises Cloudflare runtime");

run(process.execPath, [resolve(root, "scripts/prepare-toolbox.mjs")]);
const stage = resolve(root, "src-tauri/target/release-stage");
const stagedBroker = resolve(stage, "localbridge-privileged-broker.exe");
rmSync(stage, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });
writeFileSync(stagedBroker, Buffer.alloc(0));
run("cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--locked", "--release", "--bin", "localbridge-privileged-broker"]);

const broker = resolve(root, "src-tauri/target/release/localbridge-privileged-broker.exe");
if (!existsSync(broker)) throw new Error("release privileged broker was not built");
copyFileSync(broker, stagedBroker);
if (statSync(stagedBroker).size === 0) throw new Error("release privileged broker staging remained a placeholder");
console.log("LB018_RELEASE_RESOURCES=PASS broker=release runtime=install-root toolbox=pinned cloudflared=false");
