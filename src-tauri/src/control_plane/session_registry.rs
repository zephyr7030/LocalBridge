use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{McpSessionId, PublicSessionId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionRecord {
    pub id: McpSessionId,
    pub protocol: String,
    pub tool_catalog_signature: String,
    pub tools_list_changed_pending: bool,
    pub created_at_ms: u64,
    pub last_seen_ms: u64,
    pub owned_public_sessions: HashSet<PublicSessionId>,
}

impl SessionRecord {
    pub(crate) fn new(id: McpSessionId, protocol: String, tool_catalog_signature: String) -> Self {
        let now = now_unix_ms();
        Self {
            id,
            protocol,
            tool_catalog_signature,
            tools_list_changed_pending: true,
            created_at_ms: now,
            last_seen_ms: now,
            owned_public_sessions: HashSet::new(),
        }
    }

    pub(crate) fn touch(&mut self) {
        self.last_seen_ms = now_unix_ms();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionInsertError {
    Capacity,
    AlreadyExists,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionRegistry(Arc<Mutex<HashMap<McpSessionId, SessionRecord>>>);

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

    pub(crate) fn remove(&self, id: &McpSessionId) -> Option<SessionRecord> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id)
    }

    pub(crate) fn add_public_session(
        &self,
        owner: &McpSessionId,
        public_session_id: PublicSessionId,
    ) -> bool {
        self.update(owner, |session| {
            session.owned_public_sessions.insert(public_session_id)
        })
        .unwrap_or(false)
    }

    pub(crate) fn owns_public_session(
        &self,
        owner: &McpSessionId,
        public_session_id: &PublicSessionId,
    ) -> bool {
        self.get(owner)
            .is_some_and(|session| session.owned_public_sessions.contains(public_session_id))
    }

    pub(crate) fn public_sessions_owned_by(&self, owner: &McpSessionId) -> Vec<PublicSessionId> {
        self.get(owner)
            .map(|session| session.owned_public_sessions.into_iter().collect())
            .unwrap_or_default()
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
        session.touch();
        Some(update(session))
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
    fn public_session_ownership_is_scoped_by_mcp_session() {
        let registry = SessionRegistry::default();
        let owner_a = McpSessionId::new("session-a");
        let owner_b = McpSessionId::new("session-b");
        let public = PublicSessionId::new("public-1");
        registry
            .insert_bounded(record("session-a", "v1", "catalog"), 64)
            .unwrap();
        registry
            .insert_bounded(record("session-b", "v1", "catalog"), 64)
            .unwrap();

        assert!(registry.add_public_session(&owner_a, public.clone()));
        assert!(registry.owns_public_session(&owner_a, &public));
        assert!(!registry.owns_public_session(&owner_b, &public));
        assert!(registry.public_sessions_owned_by(&owner_b).is_empty());
    }
}
