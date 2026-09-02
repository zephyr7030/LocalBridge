use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{McpSessionId, McpSessionState, RequestKey};

pub(crate) const MCP_SESSION_TTL_MS: u64 = 5 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionRecord {
    pub id: McpSessionId,
    pub state: McpSessionState,
    pub protocol: String,
    pub tool_catalog_signature: String,
    pub tools_list_changed_pending: bool,
    pub created_at_ms: u64,
    pub last_seen_ms: u64,
    pub owned_requests: HashSet<RequestKey>,
}

impl SessionRecord {
    pub(crate) fn new(id: McpSessionId, protocol: String, tool_catalog_signature: String) -> Self {
        let now = now_unix_ms();
        Self {
            id,
            state: McpSessionState::Created,
            protocol,
            tool_catalog_signature,
            tools_list_changed_pending: true,
            created_at_ms: now,
            last_seen_ms: now,
            owned_requests: HashSet::new(),
        }
    }

    pub(crate) fn touch(&mut self) {
        self.last_seen_ms = now_unix_ms();
        if self.state == McpSessionState::Created {
            self.state = McpSessionState::Active;
        }
    }

    fn begin_closing(&mut self) {
        if self.state.accepts_requests() {
            self.state = McpSessionState::Closing;
        }
    }

    fn close(&mut self) {
        self.state = McpSessionState::Closed;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionInsertError {
    Capacity,
    AlreadyExists,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionRegistry(Arc<Mutex<HashMap<McpSessionId, SessionRecord>>>);

#[derive(Debug, Clone)]
pub(crate) struct SessionReaper {
    registry: SessionRegistry,
    ttl_ms: u64,
}

impl SessionReaper {
    pub(crate) fn new(registry: SessionRegistry, ttl_ms: u64) -> Self {
        Self { registry, ttl_ms }
    }

    pub(crate) fn reap_expired(&self) -> Vec<SessionRecord> {
        self.reap_at(now_unix_ms())
    }

    pub(crate) fn reap_at(&self, now_ms: u64) -> Vec<SessionRecord> {
        self.registry.reap_expired_at(now_ms, self.ttl_ms)
    }
}

impl SessionRegistry {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    pub(crate) fn insert_bounded(
        &self,
        session: SessionRecord,
        max_sessions: usize,
    ) -> Result<(), SessionInsertError> {
        let mut sessions = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sessions.len() >= max_sessions {
            return Err(SessionInsertError::Capacity);
        }
        match sessions.entry(session.id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(session);
                Ok(())
            }
            Entry::Occupied(_) => Err(SessionInsertError::AlreadyExists),
        }
    }

    pub(crate) fn get(&self, id: &McpSessionId) -> Option<SessionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    pub(crate) fn close_and_remove(&self, id: &McpSessionId) -> Option<SessionRecord> {
        let mut sessions = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let session = sessions.get_mut(id)?;
        session.begin_closing();
        session.close();
        sessions.remove(id)
    }

    pub(crate) fn add_request(&self, owner: &McpSessionId, request: RequestKey) -> bool {
        self.update(owner, |session| session.owned_requests.insert(request))
            .unwrap_or(false)
    }

    pub(crate) fn remove_request(&self, owner: &McpSessionId, request: &RequestKey) -> bool {
        self.update(owner, |session| session.owned_requests.remove(request))
            .unwrap_or(false)
    }

    pub(crate) fn update<R>(
        &self,
        id: &McpSessionId,
        update: impl FnOnce(&mut SessionRecord) -> R,
    ) -> Option<R> {
        let mut sessions = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let session = sessions.get_mut(id)?;
        if !session.state.accepts_requests() {
            return None;
        }
        session.touch();
        Some(update(session))
    }

    fn reap_expired_at(&self, now_ms: u64, ttl_ms: u64) -> Vec<SessionRecord> {
        let mut sessions = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let expired = sessions
            .iter()
            .filter(|(_, session)| {
                session.state.accepts_requests()
                    && session.owned_requests.is_empty()
                    && now_ms.saturating_sub(session.last_seen_ms) >= ttl_ms
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        expired
            .into_iter()
            .filter_map(|id| {
                let session = sessions.get_mut(&id)?;
                session.begin_closing();
                session.close();
                sessions.remove(&id)
            })
            .collect()
    }

    #[cfg(test)]
    fn set_last_seen_for_test(&self, id: &McpSessionId, last_seen_ms: u64) {
        if let Some(session) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(id)
        {
            session.last_seen_ms = last_seen_ms;
        }
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, protocol: &str, catalog: &str) -> SessionRecord {
        SessionRecord::new(McpSessionId::new(id), protocol.into(), catalog.into())
    }

    #[test]
    fn registry_keys_sessions_by_typed_identity() {
        let registry = SessionRegistry::default();
        let id = McpSessionId::new("session-a");
        registry
            .insert_bounded(record("session-a", "v1", "catalog"), 64)
            .unwrap();
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.get(&id).unwrap().id, id);
    }

    #[test]
    fn duplicate_session_identity_does_not_overwrite_existing_record() {
        let registry = SessionRegistry::default();
        let id = McpSessionId::new("session-a");
        registry
            .insert_bounded(record("session-a", "v1", "first"), 64)
            .unwrap();
        assert_eq!(
            registry.insert_bounded(record("session-a", "v2", "replacement"), 64),
            Err(SessionInsertError::AlreadyExists)
        );
        let stored = registry.get(&id).unwrap();
        assert_eq!(stored.protocol, "v1");
        assert_eq!(stored.tool_catalog_signature, "first");
    }

    #[test]
    fn capacity_check_and_insert_share_one_registry_lock() {
        let registry = SessionRegistry::default();
        registry
            .insert_bounded(record("session-a", "v1", "first"), 1)
            .unwrap();
        assert_eq!(
            registry.insert_bounded(record("session-b", "v1", "second"), 1),
            Err(SessionInsertError::Capacity)
        );
        assert!(registry.get(&McpSessionId::new("session-b")).is_none());
    }

    #[test]
    fn session_lifecycle_is_created_active_closing_closed() {
        let registry = SessionRegistry::default();
        let id = McpSessionId::new("session-a");
        registry
            .insert_bounded(record("session-a", "v1", "catalog"), 64)
            .unwrap();
        assert_eq!(registry.get(&id).unwrap().state, McpSessionState::Created);
        registry.update(&id, |_| ()).unwrap();
        assert_eq!(registry.get(&id).unwrap().state, McpSessionState::Active);
        let closed = registry.close_and_remove(&id).unwrap();
        assert_eq!(closed.state, McpSessionState::Closed);
        assert!(registry.get(&id).is_none());
    }

    #[test]
    fn ttl_reaper_closes_only_expired_sessions() {
        let registry = SessionRegistry::default();
        let expired = McpSessionId::new("expired");
        let fresh = McpSessionId::new("fresh");
        registry
            .insert_bounded(record("expired", "v1", "catalog"), 64)
            .unwrap();
        registry
            .insert_bounded(record("fresh", "v1", "catalog"), 64)
            .unwrap();
        registry.set_last_seen_for_test(&expired, 10);
        registry.set_last_seen_for_test(&fresh, 90);
        let reaper = SessionReaper::new(registry.clone(), 50);
        let reaped = reaper.reap_at(100);
        assert_eq!(reaped.len(), 1);
        assert_eq!(reaped[0].id, expired);
        assert_eq!(reaped[0].state, McpSessionState::Closed);
        assert!(registry.get(&fresh).is_some());
    }

    #[test]
    fn ttl_reaper_never_closes_a_session_with_an_active_request() {
        let registry = SessionRegistry::default();
        let id = McpSessionId::new("active-request");
        registry
            .insert_bounded(record("active-request", "v1", "catalog"), 64)
            .unwrap();
        let request = RequestKey::new(id.clone(), crate::domain::RpcRequestId::Number(9));
        assert!(registry.add_request(&id, request));
        registry.set_last_seen_for_test(&id, 10);
        assert!(
            SessionReaper::new(registry.clone(), 50)
                .reap_at(100)
                .is_empty()
        );
        assert!(registry.get(&id).is_some());
    }

    #[test]
    fn session_owns_only_active_request_leases() {
        let registry = SessionRegistry::default();
        let id = McpSessionId::new("session-a");
        registry
            .insert_bounded(record("session-a", "v1", "catalog"), 64)
            .unwrap();
        let request = RequestKey::new(id.clone(), crate::domain::RpcRequestId::Number(7));
        assert!(registry.add_request(&id, request.clone()));
        let session = registry.get(&id).unwrap();
        assert!(session.owned_requests.contains(&request));
        assert!(registry.remove_request(&id, &request));
        assert!(!registry.get(&id).unwrap().owned_requests.contains(&request));
    }
}
