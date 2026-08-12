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
  "exhausted_generation: Option<ExhaustedGeneration>",
  "if exhausted.generation == generation",
  "self.exhausted_generation = Some(ExhaustedGeneration",
]) if (!recovery.includes(required)) throw new Error(`ARCH-022 missing generation lifecycle: ${required}`);
const autoStart = recovery.indexOf("pub fn recover_auto");
const autoEnd = recovery.indexOf("pub fn manual_retry", autoStart);
const auto = recovery.slice(autoStart, autoEnd);
if (!auto.includes("return RecoveryOutcome::Exhausted")) throw new Error("ARCH-022 same exhausted generation is not terminal for automatic recovery");
const manualStart = recovery.indexOf("pub fn manual_retry");
const manualEnd = recovery.indexOf("pub fn observe_stable_ready", manualStart);
const manual = recovery.slice(manualStart, manualEnd);
if (manualStart < 0 || manualEnd <= manualStart || !manual.includes("runtime.begin_outage") || !manual.includes("self.exhausted_generation = None")) throw new Error("ARCH-022 manual retry does not create a fresh retry-budget generation");
const exhaustionMarks = [...recovery.matchAll(/runtime\.mark_user_attention_required\(generation\)/g)].length;
if (exhaustionMarks !== 6) throw new Error(`ARCH-022 expected sync+cooperative initial/post-attempt/exhausted attention call sites, got ${exhaustionMarks}`);
const cooperativeStart = recovery.indexOf("fn begin_cooperative_auto");
const cooperativeEnd = recovery.indexOf("impl<C: RecoveryClock> RecoveryController", cooperativeStart);
const cooperative = recovery.slice(cooperativeStart, cooperativeEnd);
if (!/self\s*\.\s*controller\s*\.\s*begin_or_refresh_generation\s*\(/s.test(cooperative)) throw new Error("ARCH-022 cooperative recovery bypasses the single generation owner");
if (!cooperative.includes("self.controller.exhausted_generation = Some(ExhaustedGeneration")) throw new Error("ARCH-022 cooperative exhaustion is not terminalized on the shared controller");
if (!recovery.includes("self.cancellation.cancel();\n        self.pending_auto = None;")) throw new Error("ARCH-022 manual retry does not cancel cooperative generation before creating a fresh budget");
console.log("ARCH-022_VERIFY=PASS single_active_generation=true sync_and_cooperative_exhaustion_terminal=true duplicate_attention_suppressed=true manual_retry_fresh_generation=true cooperative_cancel_before_manual=true stable_reset_60s=true");
