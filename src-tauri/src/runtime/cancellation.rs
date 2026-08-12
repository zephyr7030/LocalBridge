use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Default)]
pub struct RecoveryCancellation {
    epoch: Arc<AtomicU64>,
}

impl RecoveryCancellation {
    pub fn permit(&self) -> RecoveryPermit {
        RecoveryPermit {
            epoch: Arc::clone(&self.epoch),
            captured: self.epoch.load(Ordering::Acquire),
        }
    }

    pub fn cancel(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Clone)]
pub struct RecoveryPermit {
    epoch: Arc<AtomicU64>,
    captured: u64,
}

impl RecoveryPermit {
    pub fn is_cancelled(&self) -> bool {
        self.epoch.load(Ordering::Acquire) != self.captured
    }
}
