import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

const destDir = join("src-tauri", "binaries");
mkdirSync(destDir, { recursive: true });
const dest = join(destDir, "dummy-sidecar-x86_64-pc-windows-msvc.exe");
const rustc = spawnSync("rustc", ["src-tauri/src/bin/dummy-sidecar.rs", "-O", "-o", dest], { stdio: "inherit" });
if (rustc.status !== 0) process.exit(rustc.status ?? 1);
if (!existsSync(dest)) throw new Error("dummy sidecar executable missing after rustc");
const sha256 = createHash("sha256").update(readFileSync(dest)).digest("hex");
console.log(`DUMMY_SIDECAR_SHA256=${sha256}`);
