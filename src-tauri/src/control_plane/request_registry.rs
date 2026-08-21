use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::domain::{McpSessionId, RequestKey, RpcRequestId};
use crate::mcp::filesystem_service::FilesystemCancellation;

#[derive(Debug, Clone)]
pub(crate) enum RequestCancellationTarget {
    Runtime(RpcRequestId),
    WorkspaceFilesystem(FilesystemCancellation),
    PrivilegedExecution(String),
    PrivilegedFilesystem(String),
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveRequest {
    pub key: RequestKey,
    pub cancellation: RequestCancellationTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestRegistryError {
    AlreadyActive,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RequestRegistry(Arc<Mutex<HashMap<RequestKey, ActiveRequest>>>);

impl RequestRegistry {
    pub(crate) fn register(
        &self,
        key: RequestKey,
        cancellation: RequestCancellationTarget,
    ) -> Result<(), RequestRegistryError> {
        let mut requests = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if requests.contains_key(&key) {
            return Err(RequestRegistryError::AlreadyActive);
        }
        requests.insert(key.clone(), ActiveRequest { key, cancellation });
        Ok(())
    }

    pub(crate) fn remove(&self, key: &RequestKey) -> Option<ActiveRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(key)
    }

    pub(crate) fn get(&self, key: &RequestKey) -> Option<ActiveRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, key: &RequestKey) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(key)
    }

    pub(crate) fn owned_by(&self, session_id: &McpSessionId) -> Vec<ActiveRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|request| &request.key.session_id == session_id)
            .cloned()
            .collect()
    }

    pub(crate) fn all(&self) -> Vec<ActiveRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(session: &str, id: i64) -> RequestKey {
        RequestKey::new(McpSessionId::new(session), RpcRequestId::Number(id))
    }

    fn runtime_target(id: i64) -> RequestCancellationTarget {
        RequestCancellationTarget::Runtime(RpcRequestId::Number(id))
    }

    #[test]
    fn equal_rpc_ids_in_distinct_sessions_are_independent() {
        let registry = RequestRegistry::default();
        let a = key("a", 1);
        let b = key("b", 1);
        registry.register(a.clone(), runtime_target(101)).unwrap();
        registry.register(b.clone(), runtime_target(102)).unwrap();

        assert!(registry.remove(&a).is_some());
        assert!(!registry.contains(&a));
        assert!(registry.contains(&b));
    }

    #[test]
    fn ownership_query_never_returns_another_session() {
        let registry = RequestRegistry::default();
        registry.register(key("a", 1), runtime_target(101)).unwrap();
        registry.register(key("b", 1), runtime_target(102)).unwrap();

        let owned = registry.owned_by(&McpSessionId::new("a"));
        assert_eq!(owned.len(), 1);
        assert_eq!(owned[0].key.session_id.as_str(), "a");
    }

    #[test]
    fn duplicate_active_request_key_is_rejected_without_overwrite() {
        let registry = RequestRegistry::default();
        let key = key("a", 1);
        registry.register(key.clone(), runtime_target(101)).unwrap();
        assert_eq!(
            registry.register(key.clone(), runtime_target(102)),
            Err(RequestRegistryError::AlreadyActive)
        );
        let active = registry.get(&key).unwrap();
        assert!(matches!(
            active.cancellation,
            RequestCancellationTarget::Runtime(RpcRequestId::Number(101))
        ));
    }
}
