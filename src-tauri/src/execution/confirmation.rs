//! Holds administrator commands that the risk classifier flagged, until a
//! person says yes.
//!
//! The shape is deliberately the same as `command_control`'s adoption token:
//! a CSPRNG value handed out once, stored only as a SHA-256 digest, and spent
//! on redemption. What it adds is that possession of the token is *not*
//! permission. The model receives the token immediately, but the token stays
//! unredeemable until someone approves the request in the window. That keeps
//! the MCP call non-blocking — it returns at once instead of parking a worker
//! thread on a human — while still putting a person between the model and an
//! irreversible command.
//!
//! The digest covers the route, the command text and the working directory
//! together, so an approved token cannot be replayed against a different
//! command. Approving `del C:\temp\*` does not approve `del C:\*`.

use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// How long a request waits for a person before it lapses. Long enough to walk
/// back to the machine, short enough that a forgotten prompt is not a standing
/// permission.
const AWAITING_TTL_MS: u64 = 10 * 60 * 1000;

/// How long an approval stays spendable. An approval is for the command in
/// front of the user now, not for whatever the model gets around to later.
const APPROVED_TTL_MS: u64 = 5 * 60 * 1000;

/// A model that keeps asking must not be able to grow this without bound. The
/// oldest awaiting entries are dropped first; an approved entry is never
/// evicted by pressure, only by expiry or redemption.
const MAX_PENDING: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Awaiting,
    Approved,
}

/// Why a redemption did not run the command. Every variant means "did not
/// execute"; they differ only in what the caller should be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedeemError {
    /// No such token, or it was already spent. Both look identical from
    /// outside on purpose.
    Unknown,
    /// The token exists but nobody has approved it yet.
    Awaiting,
    /// The token was approved, but for a different command, directory or route.
    Mismatch,
    /// The token aged out.
    Expired,
}

impl RedeemError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Awaiting => "awaiting",
            Self::Mismatch => "mismatch",
            Self::Expired => "expired",
        }
    }
}

/// What the window needs to show a person so they can decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingConfirmation {
    pub id: String,
    pub route: String,
    pub command: String,
    pub workdir: Option<String>,
    pub risk: Vec<String>,
    pub requested_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
struct Entry {
    id: String,
    token_hash: String,
    request_hash: String,
    route: String,
    command: String,
    workdir: Option<String>,
    risk: Vec<String>,
    decision: Decision,
    requested_at_ms: u64,
    /// Recomputed when the decision changes, so an approval gets its own,
    /// shorter clock rather than inheriting the waiting one.
    expires_at_ms: u64,
}

#[derive(Default)]
struct Store {
    entries: Vec<Entry>,
    /// Bumped on every change so the UI projection can tell it needs to redraw
    /// without diffing the list.
    revision: u64,
}

#[derive(Default)]
struct ConfirmationStore {
    state: Mutex<Store>,
    changed: Condvar,
}

static STORE: OnceLock<ConfirmationStore> = OnceLock::new();

fn confirmation_store() -> &'static ConfirmationStore {
    STORE.get_or_init(ConfirmationStore::default)
}

fn store() -> MutexGuard<'static, Store> {
    confirmation_store()
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn notify_changed() {
    confirmation_store().changed.notify_all();
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

/// Binds the approval to the exact request. The separator cannot appear in a
/// path or a command, so `("a", "b\u{1}c")` and `("a\u{1}b", "c")` cannot
/// collide into the same digest.
fn request_digest(route: &str, command: &str, workdir: Option<&str>) -> String {
    digest(&format!(
        "{route}\u{1}{command}\u{1}{}",
        workdir.unwrap_or_default()
    ))
}

fn prune(store: &mut Store, now: u64) -> bool {
    let before = store.entries.len();
    store.entries.retain(|entry| entry.expires_at_ms > now);
    let changed = store.entries.len() != before;
    if changed {
        store.revision += 1;
    }
    changed
}

/// Records a flagged command and returns the token the model must come back
/// with. The token is returned once and never stored in the clear.
pub fn request(
    route: &str,
    command: &str,
    workdir: Option<&str>,
    risk: &[&'static str],
) -> (String, String) {
    let token = crate::security::random_prefixed_id("lb-confirm-");
    let id = crate::security::random_prefixed_id("lb-cfm-");
    let now = now_ms();
    let mut store = store();
    prune(&mut store, now);

    // Pressure evicts the oldest thing still waiting on a person; anything
    // already approved is a decision that was made and is left alone.
    while store.entries.len() >= MAX_PENDING {
        let Some(index) = store
            .entries
            .iter()
            .position(|entry| entry.decision == Decision::Awaiting)
        else {
            break;
        };
        store.entries.remove(index);
    }

    store.entries.push(Entry {
        id: id.clone(),
        token_hash: digest(&token),
        request_hash: request_digest(route, command, workdir),
        route: route.to_string(),
        command: command.to_string(),
        workdir: workdir.map(str::to_string),
        risk: risk.iter().copied().map(String::from).collect(),
        decision: Decision::Awaiting,
        requested_at_ms: now,
        expires_at_ms: now.saturating_add(AWAITING_TTL_MS),
    });
    store.revision += 1;
    drop(store);
    notify_changed();
    (token, id)
}

/// Spends the token. Succeeds only for an approved, unexpired token whose
/// digest still matches the command being asked for. Success removes the
/// entry, so one approval runs one command exactly once.
pub fn redeem(
    token: &str,
    route: &str,
    command: &str,
    workdir: Option<&str>,
) -> Result<(), RedeemError> {
    let now = now_ms();
    let mut store = store();
    // 这里刻意不先 prune：先清理再查表的话，过期的条目在查之前就没了，
    // 调用方永远收到"令牌不存在"——那会让模型以为是自己记错了，而实际上
    // 只是人太久没批。过期与不存在是两件事，得分开说。
    let hash = digest(token);
    let Some(index) = store
        .entries
        .iter()
        .position(|entry| entry.token_hash == hash)
    else {
        return Err(RedeemError::Unknown);
    };
    if store.entries[index].expires_at_ms <= now {
        store.entries.remove(index);
        store.revision += 1;
        drop(store);
        notify_changed();
        return Err(RedeemError::Expired);
    }
    if store.entries[index].decision != Decision::Approved {
        return Err(RedeemError::Awaiting);
    }
    // Checked after approval so that a mismatch cannot be used to probe which
    // tokens exist and which are already approved.
    if store.entries[index].request_hash != request_digest(route, command, workdir) {
        return Err(RedeemError::Mismatch);
    }
    store.entries.remove(index);
    store.revision += 1;
    drop(store);
    notify_changed();
    Ok(())
}

/// The person said yes. Restarts the clock on the shorter approved budget.
pub fn approve(id: &str) -> bool {
    let now = now_ms();
    let mut store = store();
    let pruned = prune(&mut store, now);
    let Some(entry) = store.entries.iter_mut().find(|entry| entry.id == id) else {
        drop(store);
        if pruned {
            notify_changed();
        }
        return false;
    };
    entry.decision = Decision::Approved;
    entry.expires_at_ms = now.saturating_add(APPROVED_TTL_MS);
    store.revision += 1;
    drop(store);
    notify_changed();
    true
}

/// The person said no. The entry goes away; the model's retry will report the
/// token as unknown, which is exactly what it now is.
pub fn reject(id: &str) -> bool {
    let mut store = store();
    let before = store.entries.len();
    store.entries.retain(|entry| entry.id != id);
    let removed = store.entries.len() != before;
    if removed {
        store.revision += 1;
    }
    drop(store);
    if removed {
        notify_changed();
    }
    removed
}

/// Everything still waiting on a person, oldest first — the order someone
/// would work through them.
pub fn awaiting() -> Vec<PendingConfirmation> {
    let now = now_ms();
    let mut store = store();
    let pruned = prune(&mut store, now);
    let pending = store
        .entries
        .iter()
        .filter(|entry| entry.decision == Decision::Awaiting)
        .map(|entry| PendingConfirmation {
            id: entry.id.clone(),
            route: entry.route.clone(),
            command: entry.command.clone(),
            workdir: entry.workdir.clone(),
            risk: entry.risk.clone(),
            requested_at_ms: entry.requested_at_ms,
            expires_at_ms: entry.expires_at_ms,
        })
        .collect();
    drop(store);
    if pruned {
        notify_changed();
    }
    pending
}

pub fn revision() -> u64 {
    store().revision
}

pub fn wait_for_revision_after(since_revision: u64, timeout: Duration) -> u64 {
    let holder = confirmation_store();
    let deadline = Instant::now() + timeout;
    let mut store = holder.state.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        let now = now_ms();
        if prune(&mut store, now) {
            holder.changed.notify_all();
        }
        if store.revision > since_revision {
            return store.revision;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return store.revision;
        }
        let expiry_wait = store
            .entries
            .iter()
            .filter(|entry| entry.decision == Decision::Awaiting)
            .map(|entry| Duration::from_millis(entry.expires_at_ms.saturating_sub(now)))
            .min()
            .unwrap_or(remaining);
        let wait_for = remaining.min(expiry_wait);
        let (next, _) = holder
            .changed
            .wait_timeout(store, wait_for)
            .unwrap_or_else(PoisonError::into_inner);
        store = next;
    }
}

/// 这些用例共用一个进程级存储，而 cargo 默认并行跑测试。这把锁归模块所有
/// 而不是归某一个测试文件，因为 `server_tests` 里的闸门用例也要排进同一条队。
#[cfg(test)]
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_for_test() -> MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    reset_for_test();
    guard
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    let mut store = store();
    store.entries.clear();
    store.revision = 0;
    drop(store);
    notify_changed();
}

#[cfg(test)]
pub(crate) fn force_expiry_for_test(id: &str) {
    let mut store = store();
    if let Some(entry) = store.entries.iter_mut().find(|entry| entry.id == id) {
        entry.expires_at_ms = 0;
    }
    drop(store);
    notify_changed();
}

#[cfg(test)]
#[path = "confirmation_tests.rs"]
mod confirmation_tests;
