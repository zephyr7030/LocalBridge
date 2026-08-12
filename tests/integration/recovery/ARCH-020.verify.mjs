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
for (const required of [
  "RuntimeOutage::classify(component, final_fault.clone())",
  "classified.disposition == RecoveryDisposition::NonRecoverable",
  "return RecoveryOutcome::NonRecoverable",
]) if (!loop.includes(required)) throw new Error(`ARCH-020 missing post-attempt typed retryability gate: ${required}`);
const afterLoop = recovery.slice(loopEnd, recovery.indexOf("RecoveryOutcome::Exhausted", loopEnd) + 200);
if (!afterLoop.includes("runtime.mark_user_attention_required(generation)")) throw new Error("ARCH-020 missing final attention after fifth failure");
const cooperativeStart = recovery.indexOf("fn begin_cooperative_auto");
const cooperativeAdvanceStart = recovery.indexOf("fn advance_pending_auto", cooperativeStart);
const cooperativeEnd = recovery.indexOf("impl<C: RecoveryClock> RecoveryController", cooperativeAdvanceStart);
if (cooperativeStart < 0 || cooperativeAdvanceStart <= cooperativeStart || cooperativeEnd <= cooperativeAdvanceStart) throw new Error("ARCH-020 cooperative automatic recovery state machine missing");
const cooperative = recovery.slice(cooperativeStart, cooperativeEnd);
for (const required of [
  "PendingAutoRecovery",
  "next_deadline",
  "RECONNECT_BACKOFF_SECONDS[0]",
  "RECONNECT_BACKOFF_SECONDS[(next_attempt - 1) as usize]",
  "recover_minimal_cancellable(scope, attempt, &permit)",
  "attempt >= RECONNECT_BACKOFF_SECONDS.len() as u32",
  "classified.disposition == RecoveryDisposition::NonRecoverable",
  "permit.is_cancelled()",
  "set_permission_mode_after_control_cancellation",
  "resume_after_control_interruption",
  "pending.permit = fresh_permit",
  "next_attempt: attempt",
]) if (!recovery.includes(required)) throw new Error(`ARCH-020 missing cooperative reconnect invariant: ${required}`);
if (cooperative.includes(".sleep(")) throw new Error("ARCH-020 cooperative automatic recovery must use deadlines, not blocking sleep");
const cancellableCalls = (cooperative.match(/recover_minimal_cancellable/g) ?? []).length;
if (cancellableCalls !== 1) throw new Error(`ARCH-020 cooperative monitor must execute at most one reconnect attempt per advancement, found ${cancellableCalls} call sites`);
for (const required of ["RecoveryScope::Tunnel", "RecoveryScope::PolicyAndTunnel", "RecoveryScope::FullRuntime", "confirm_pep_ready", "confirm_mcp_ready"]) if (!orchestrator.includes(required)) process.exit(5);
for (const required of ["confirm_mcp_ready_for_recovery", "confirm_pep_ready_for_recovery", "confirm_tunnel_ready_for_recovery", "RecoveryPermit"]) if (!orchestrator.includes(required)) throw new Error(`ARCH-020 cancellable recovery stage missing: ${required}`);
const cancellableStart = orchestrator.indexOf("pub fn recover_minimal_cancellable");
const cancellableEnd = orchestrator.indexOf("pub fn switch_workspace_to", cancellableStart);
if (cancellableStart < 0 || cancellableEnd <= cancellableStart) throw new Error("ARCH-020 cancellable recovery implementation missing");
const cancellable = orchestrator.slice(cancellableStart, cancellableEnd);
for (const required of [
  "permit.is_cancelled()",
  "error.fault == RuntimeFault::UserStopped",
  "error.cleanup_fault.is_none()",
  "self.state = RuntimeState::Recovering { component, attempt }",
]) if (!cancellable.includes(required)) throw new Error(`ARCH-020 control cancellation can terminalize cooperative recovery: ${required}`);
if (/while\s*\([^)]*reconnect|loop\s*\{[\s\S]{0,200}recover_minimal/.test(recovery)) throw new Error("ARCH-020 unbounded reconnect loop detected");
console.log("ARCH-020_VERIFY=PASS sync_attempts=5 cooperative_deadlines=1,2,5,10,30 cooperative_sleep=false post_attempt_retryability=true cancellable=true control_interrupt_resume=true same_generation_exhaustion_terminal=true minimal_layer_health_gate=true");
