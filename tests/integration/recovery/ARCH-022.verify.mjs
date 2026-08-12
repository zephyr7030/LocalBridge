import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(process.env.LOCALBRIDGE_REPO_ROOT ?? ".");
if (process.env.LOCALBRIDGE_ARCH_RULE_ID !== "ARCH-022" || process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR !== "LB-010") process.exit(2);
const recoveryPath = join(root, "src-tauri", "src", "runtime", "recovery.rs");
const orchestratorPath = join(root, "src-tauri", "src", "runtime", "orchestrator.rs");
if (!existsSync(recoveryPath) || !existsSync(orchestratorPath)) process.exit(3);
const recovery = readFileSync(recoveryPath, "utf8");
const orchestrator = readFileSync(orchestratorPath, "utf8");
for (const required of [
  "active: Option<OutageGeneration>",
  "user_attention_emitted: bool",
  "if active.user_attention_emitted",
  "active.user_attention_emitted = true",
  "pub fn refresh(",
  "pub fn clear(",
]) if (!orchestrator.includes(required)) throw new Error(`ARCH-022 missing outage generation invariant: ${required}`);
if (/active:\s*Vec\s*</.test(orchestrator)) throw new Error("ARCH-022 outage tracker accumulates generations");
for (const required of [
  "if !runtime.refresh_outage(generation, outage.component, outage.fault.clone())",
  "self.generation = Some(fresh)",
  "let generation = runtime.begin_outage(outage.component, outage.fault.clone())",
  "pub fn manual_retry",
  "pub const STABILITY_RESET_SECONDS: u64 = 60",
  "runtime.clear_outage(generation)",
]) if (!recovery.includes(required)) throw new Error(`ARCH-022 missing generation lifecycle: ${required}`);
const manualStart = recovery.indexOf("pub fn manual_retry");
const manualEnd = recovery.indexOf("pub fn observe_stable_ready", manualStart);
if (manualStart < 0 || manualEnd <= manualStart || !recovery.slice(manualStart, manualEnd).includes("runtime.begin_outage")) throw new Error("ARCH-022 manual retry does not create fresh generation");
const exhaustionMarks = [...recovery.matchAll(/runtime\.mark_user_attention_required\(generation\)/g)].length;
if (exhaustionMarks !== 2) throw new Error(`ARCH-022 expected one nonretryable and one exhausted attention call site, got ${exhaustionMarks}`);
console.log("ARCH-022_VERIFY=PASS single_active_generation=true duplicate_attention_suppressed=true manual_retry_fresh_generation=true stable_reset_60s=true");
