import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const target = join(root, "src-tauri", "target");
const stage = join(target, "toolbox-stage");
const cache = join(target, "toolbox-downloads");
const extract = join(target, "toolbox-extract");

const tools = {
  aria2c: {
    url: "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2-1.37.0-win-64bit-build1.zip",
    archive: "aria2-1.37.0-win-64bit-build1.zip",
    archiveSha256: "67d015301eef0b612191212d564c5bb0a14b5b9c4796b76454276a4d28d9b288",
    executableSha256: "be2099c214f63a3cb4954b09a0becd6e2e34660b886d4c898d260febfe9d70c2",
  },
  sevenZip: {
    url: "https://www.7-zip.org/a/7z2602-extra.7z",
    archive: "7z2602-extra.7z",
    archiveSha256: "081df9e9311dfd9c9e0e98c1c80180b99bb51e4cb24156b5f3057fe3c259d70a",
    executableSha256: "35d4d69d7cd6cb44558f208c3b1334268013f9daf82d2dda848893a1c30c59c2",
  },
  jq: {
    url: "https://github.com/jqlang/jq/releases/download/jq-1.8.2/jq-windows-amd64.exe",
    archive: "jq-windows-amd64.exe",
    archiveSha256: "a6fc67fedaf9128a3309a1e2ebb8b986aeccf70122ee46d2cb4849e423f0c627",
    executableSha256: "a6fc67fedaf9128a3309a1e2ebb8b986aeccf70122ee46d2cb4849e423f0c627",
  },
};

const sha256 = async (path) => createHash("sha256").update(await readFile(path)).digest("hex");
const valid = async (path, expected) => existsSync(path) && (await sha256(path)) === expected;
const run = (program, args) => {
  const result = spawnSync(program, args, { stdio: "inherit", windowsHide: true });
  if (result.status !== 0) throw new Error(`${program} failed with exit ${result.status ?? "unknown"}`);
};
const download = async (tool) => {
  await mkdir(cache, { recursive: true });
  const path = join(cache, tool.archive);
  if (await valid(path, tool.archiveSha256)) return path;
  const response = await fetch(tool.url, { redirect: "follow" });
  if (!response.ok) throw new Error(`toolbox download failed: ${response.status} ${tool.url}`);
  await writeFile(path, Buffer.from(await response.arrayBuffer()));
  if (!(await valid(path, tool.archiveSha256))) throw new Error(`toolbox SHA256 mismatch: ${tool.archive}`);
  return path;
};
const install = async (destination, expected, prepare) => {
  if (await valid(destination, expected)) return;
  await mkdir(dirname(destination), { recursive: true });
  await prepare(destination);
  if (!(await valid(destination, expected))) throw new Error(`toolbox executable SHA256 mismatch: ${destination}`);
};

await rm(stage, { recursive: true, force: true });
const bin = join(stage, "bin");
await mkdir(bin, { recursive: true });
await install(join(bin, "aria2c.exe"), tools.aria2c.executableSha256, async (destination) => {
  const archive = await download(tools.aria2c);
  const out = join(extract, "aria2c");
  await rm(out, { recursive: true, force: true });
  await mkdir(out, { recursive: true });
  run("tar.exe", ["-xf", archive, "-C", out]);
  await copyFile(join(out, "aria2-1.37.0-win-64bit-build1", "aria2c.exe"), destination);
});
await install(join(bin, "7z.exe"), tools.sevenZip.executableSha256, async (destination) => {
  const archive = await download(tools.sevenZip);
  const out = join(extract, "7z");
  await rm(out, { recursive: true, force: true });
  await mkdir(out, { recursive: true });
  run("tar.exe", ["-xf", archive, "-C", out, "x64/7za.exe"]);
  await copyFile(join(out, "x64", "7za.exe"), destination);
});
await install(join(bin, "jq.exe"), tools.jq.executableSha256, async (destination) => {
  await copyFile(await download(tools.jq), destination);
});

await writeFile(join(bin, "curl.cmd"), "@echo off\r\n\"%SystemRoot%\\System32\\curl.exe\" %*\r\n", "utf8");
console.log("TOOLBOX_PREPARE=PASS aria2c=1.37.0 7z=26.02 jq=1.8.2 runtime_download=false");
