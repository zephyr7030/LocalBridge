import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(process.env.LOCALBRIDGE_REPO_ROOT ?? ".");
if (process.env.LOCALBRIDGE_ARCH_RULE_ID !== "ARCH-020" || process.env.LOCALBRIDGE_ARCH_ACTIVATE_AT_PR !== "LB-010") process.exit(2);
const recoveryPath = join(root, "src-tauri", "src", "runtime", "recovery.rs");
const orchestratorPath = join(root, "src-tauri", "src", "runtime", "orchestrator.rs");
if (!existsSync(recoveryPath) || !existsSync(orchestratorPath)) process.exit(3);
const recovery = readFileSync(recoveryPath, "utf8");
const orchestrator = readFileSync(orchestratorPath, "utf8");
for (const required of [
  "pub const RECONNECT_BACKOFF_SECONDS: [u64; 5] = [1, 2, 5, 10, 30]",
  "for (index, seconds) in RECONNECT_BACKOFF_SECONDS.into_iter().enumerate()",
  "self.current_attempt = (index + 1) as u32",
  "self.clock.sleep(Duration::from_secs(seconds))",
  "runtime.recover_minimal(scope, self.current_attempt)",
  "pub const fn classify(component: RuntimeComponent, fault: RuntimeFault) -> Self",
  "RecoveryDisposition::NonRecoverable",
  "RuntimeFault::TunnelAuthFailed",
  "RuntimeFault::RuntimeKeyMissing",
  "RuntimeFault::RuntimeChecksumMismatch",
  "RuntimeFault::SecretInjectionUnsupported",
  "RuntimeFault::ConfigurationInvalid",
  "exhausted_generation: Option<ExhaustedGeneration>",
  "if exhausted.generation == generation",
  "self.exhausted_generation = Some(ExhaustedGeneration",
]) if (!recovery.includes(required)) throw new Error(`ARCH-020 missing reconnect contract: ${required}`);
const autoStart = recovery.indexOf("pub fn recover_auto");
const autoEnd = recovery.indexOf("pub fn manual_retry", autoStart);
if (autoStart < 0 || autoEnd <= autoStart) process.exit(4);
const auto = recovery.slice(autoStart, autoEnd);
const exhaustedGuard = auto.indexOf("if exhausted.generation == generation");
const attemptReset = auto.indexOf("self.current_attempt = 0");
const generationRun = auto.indexOf("self.run_generation(runtime, generation, outage)");
if (!(exhaustedGuard >= 0 && attemptReset > exhaustedGuard && generationRun > exhaustedGuard)) {
  throw new Error("ARCH-020 exhausted generation can reset or re-run the automatic retry budget");
}
const loopStart = recovery.indexOf("for (index, seconds) in RECONNECT_BACKOFF_SECONDS.into_iter().enumerate()");
const loopEnd = recovery.indexOf("runtime.record_fault(final_fault.clone())", loopStart);
if (loopStart < 0 || loopEnd <= loopStart) process.exit(4);
const loop = recovery.slice(loopStart, loopEnd);
if (loop.indexOf("self.clock.sleep") > loop.indexOf("runtime.recover_minimal")) throw new Error("ARCH-020 attempt backoff must precede reconnect attempt");
if (loop.includes("mark_user_attention_required")) throw new Error("ARCH-020 user attention emitted before reconnect exhaustion");
const afterLoop = recovery.slice(loopEnd, recovery.indexOf("RecoveryOutcome::Exhausted", loopEnd) + 200);
if (!afterLoop.includes("runtime.mark_user_attention_required(generation)")) throw new Error("ARCH-020 missing final attention after fifth failure");
for (const required of ["RecoveryScope::Tunnel", "RecoveryScope::PolicyAndTunnel", "RecoveryScope::FullRuntime", "confirm_pep_ready", "confirm_mcp_ready"]) if (!orchestrator.includes(required)) process.exit(5);
if (/while\s*\([^)]*reconnect|loop\s*\{[\s\S]{0,200}recover_minimal/.test(recovery)) throw new Error("ARCH-020 unbounded reconnect loop detected");
console.log("ARCH-020_VERIFY=PASS attempts=5 backoff=1,2,5,10,30 same_generation_exhaustion_terminal=true typed_retryability=true minimal_layer_health_gate=true");
