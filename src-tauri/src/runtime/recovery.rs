use std::time::{Duration, Instant};

use crate::state::{RuntimeComponent, RuntimeFault, RuntimeState};

use super::{OutageGenerationId, RecoveryScope, RuntimeDriver, RuntimeOrchestrator};

pub const RECONNECT_BACKOFF_SECONDS: [u64; 5] = [1, 2, 5, 10, 30];
pub const STABILITY_RESET_SECONDS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryDisposition {
    Recoverable,
    NonRecoverable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOutage {
    pub component: RuntimeComponent,
    pub fault: RuntimeFault,
    pub disposition: RecoveryDisposition,
}

impl RuntimeOutage {
    pub const fn classify(component: RuntimeComponent, fault: RuntimeFault) -> Self {
        let disposition = match fault {
            RuntimeFault::McpHealthTimeout
            | RuntimeFault::McpExited
            | RuntimeFault::PolicyBindFailed
            | RuntimeFault::TunnelHealthTimeout
            | RuntimeFault::TunnelExited
            | RuntimeFault::PortUnavailable => RecoveryDisposition::Recoverable,
            RuntimeFault::WorkspaceMissing
            | RuntimeFault::WorkspaceInvalid
            | RuntimeFault::RuntimeMissing
            | RuntimeFault::RuntimeChecksumMismatch
            | RuntimeFault::ProcessOwnershipFailed
            | RuntimeFault::McpSpawnFailed
            | RuntimeFault::PolicyInvalid
            | RuntimeFault::PolicyCapabilityUnknown
            | RuntimeFault::TunnelIdMissing
            | RuntimeFault::RuntimeKeyMissing
            | RuntimeFault::SecretStoreFailed
            | RuntimeFault::SecretInjectionUnsupported
            | RuntimeFault::TunnelAuthFailed
            | RuntimeFault::TunnelSpawnFailed
            | RuntimeFault::ConfigurationInvalid
            | RuntimeFault::UserStopped
            | RuntimeFault::Unknown => RecoveryDisposition::NonRecoverable,
        };
        Self { component, fault, disposition }
    }

    pub const fn recovery_scope(&self) -> RecoveryScope {
        match self.component {
            RuntimeComponent::Tunnel => RecoveryScope::Tunnel,
            RuntimeComponent::PolicyEnforcement => RecoveryScope::PolicyAndTunnel,
            RuntimeComponent::CodingRuntime => RecoveryScope::FullRuntime,
        }
    }
}

pub trait RecoveryClock {
    fn now(&self) -> Duration;
    fn sleep(&mut self, duration: Duration);
}

#[derive(Debug)]
pub struct SystemRecoveryClock {
    origin: Instant,
}

impl Default for SystemRecoveryClock {
    fn default() -> Self {
        Self { origin: Instant::now() }
    }
}

impl RecoveryClock for SystemRecoveryClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    Recovered { generation: OutageGenerationId, attempt: u32 },
    Exhausted {
        generation: OutageGenerationId,
        final_fault: RuntimeFault,
        user_attention_required: bool,
    },
    NonRecoverable {
        generation: OutageGenerationId,
        fault: RuntimeFault,
        user_attention_required: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExhaustedGeneration {
    generation: OutageGenerationId,
    final_fault: RuntimeFault,
}

#[derive(Debug)]
pub struct RecoveryController<C: RecoveryClock> {
    clock: C,
    generation: Option<OutageGenerationId>,
    stable_since: Option<Duration>,
    current_attempt: u32,
    exhausted_generation: Option<ExhaustedGeneration>,
}

impl<C: RecoveryClock> RecoveryController<C> {
    pub fn new(clock: C) -> Self {
        Self {
            clock,
            generation: None,
            stable_since: None,
            current_attempt: 0,
            exhausted_generation: None,
        }
    }

    pub const fn current_attempt(&self) -> u32 {
        self.current_attempt
    }

    pub const fn active_generation(&self) -> Option<OutageGenerationId> {
        self.generation
    }

    pub fn clock(&self) -> &C {
        &self.clock
    }

    pub fn clock_mut(&mut self) -> &mut C {
        &mut self.clock
    }

    pub fn recover_auto<D: RuntimeDriver>(
        &mut self,
        runtime: &mut RuntimeOrchestrator<D>,
        outage: RuntimeOutage,
    ) -> RecoveryOutcome {
        let generation = match self.generation {
            Some(generation) => {
                if !runtime.refresh_outage(generation, outage.component, outage.fault.clone()) {
                    let fresh = runtime.begin_outage(outage.component, outage.fault.clone());
                    self.generation = Some(fresh);
                    self.exhausted_generation = None;
                    fresh
                } else {
                    generation
                }
            }
            None => {
                let fresh = runtime.begin_outage(outage.component, outage.fault.clone());
                self.generation = Some(fresh);
                self.exhausted_generation = None;
                fresh
            }
        };
        self.stable_since = None;
        if outage.disposition == RecoveryDisposition::Recoverable {
            if let Some(exhausted) = &self.exhausted_generation {
                if exhausted.generation == generation {
                    return RecoveryOutcome::Exhausted {
                        generation,
                        final_fault: exhausted.final_fault.clone(),
                        user_attention_required: false,
                    };
                }
            }
        }
        self.current_attempt = 0;
        self.run_generation(runtime, generation, outage)
    }

    pub fn manual_retry<D: RuntimeDriver>(
        &mut self,
        runtime: &mut RuntimeOrchestrator<D>,
        outage: RuntimeOutage,
    ) -> RecoveryOutcome {
        let generation = runtime.begin_outage(outage.component, outage.fault.clone());
        self.generation = Some(generation);
        self.stable_since = None;
        self.current_attempt = 0;
        self.exhausted_generation = None;
        self.run_generation(runtime, generation, outage)
    }

    pub fn observe_stable_ready<D: RuntimeDriver>(
        &mut self,
        runtime: &mut RuntimeOrchestrator<D>,
    ) -> bool {
        let Some(generation) = self.generation else { return false; };
        if runtime.state() != &RuntimeState::Ready {
            self.stable_since = None;
            return false;
        }
        let Some(stable_since) = self.stable_since else {
            self.stable_since = Some(self.clock.now());
            return false;
        };
        if self.clock.now().saturating_sub(stable_since) < Duration::from_secs(STABILITY_RESET_SECONDS) {
            return false;
        }
        let cleared = runtime.clear_outage(generation);
        if cleared {
            self.generation = None;
            self.current_attempt = 0;
            self.stable_since = None;
            self.exhausted_generation = None;
        }
        cleared
    }

    fn run_generation<D: RuntimeDriver>(
        &mut self,
        runtime: &mut RuntimeOrchestrator<D>,
        generation: OutageGenerationId,
        outage: RuntimeOutage,
    ) -> RecoveryOutcome {
        if outage.disposition == RecoveryDisposition::NonRecoverable {
            runtime.record_fault(outage.fault.clone());
            let user_attention_required = runtime.mark_user_attention_required(generation);
            return RecoveryOutcome::NonRecoverable {
                generation,
                fault: outage.fault,
                user_attention_required,
            };
        }

        let scope = outage.recovery_scope();
        let mut final_fault = outage.fault;
        for (index, seconds) in RECONNECT_BACKOFF_SECONDS.into_iter().enumerate() {
            self.current_attempt = (index + 1) as u32;
            self.clock.sleep(Duration::from_secs(seconds));
            match runtime.recover_minimal(scope, self.current_attempt) {
                Ok(()) => {
                    let attempt = self.current_attempt;
                    self.current_attempt = 0;
                    self.stable_since = Some(self.clock.now());
                    self.exhausted_generation = None;
                    return RecoveryOutcome::Recovered { generation, attempt };
                }
                Err(error) => final_fault = error.fault,
            }
        }
        runtime.record_fault(final_fault.clone());
        let user_attention_required = runtime.mark_user_attention_required(generation);
        self.exhausted_generation = Some(ExhaustedGeneration {
            generation,
            final_fault: final_fault.clone(),
        });
        RecoveryOutcome::Exhausted { generation, final_fault, user_attention_required }
    }
}

#[cfg(test)]
mod tests {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/integration/recovery/recovery.rs"));
}
