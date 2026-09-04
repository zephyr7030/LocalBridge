use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::domain::{McpSessionId, OperationError, RequestKey, RpcRequestId};
use crate::filesystem::service::FilesystemCancellation;

const MAX_RETAINED_REQUEST_ERRORS: usize = 256;

#[derive(Debug, Clone)]
pub(crate) enum RequestCancellationTarget {
    QueuedWork,
    Runtime(RpcRequestId),
    WorkspaceFilesystem(FilesystemCancellation),
    PrivilegedFilesystem(String),
    PrivilegedExecution(String),
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveRequest {
    pub key: RequestKey,
    pub cancellation: RequestCancellationTarget,
    pub state: ActiveRequestState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveRequestState {
    Active,
    CancellationRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestRegistryError {
    AlreadyActive,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RequestRegistry(Arc<Mutex<RequestRegistryState>>);

#[derive(Debug, Default)]
struct RequestRegistryState {
    active: HashMap<RequestKey, ActiveRequest>,
    errors: VecDeque<(RequestKey, OperationError)>,
}

impl RequestRegistry {
    pub(crate) fn register(
        &self,
        key: RequestKey,
        cancellation: RequestCancellationTarget,
    ) -> Result<(), RequestRegistryError> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active.contains_key(&key) {
            return Err(RequestRegistryError::AlreadyActive);
        }
        state.active.insert(
            key.clone(),
            ActiveRequest {
                key,
                cancellation,
                state: ActiveRequestState::Active,
            },
        );
        Ok(())
    }

    pub(crate) fn replace_cancellation_target(
        &self,
        key: &RequestKey,
        cancellation: RequestCancellationTarget,
    ) -> Option<ActiveRequest> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let request = state.active.get_mut(key)?;
        request.cancellation = cancellation;
        Some(request.clone())
    }

    pub(crate) fn request_cancellation(&self, key: &RequestKey) -> Option<ActiveRequest> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let request = state.active.get_mut(key)?;
        request.state = ActiveRequestState::CancellationRequested;
        Some(request.clone())
    }

    pub(crate) fn cancellation_was_requested(&self, key: &RequestKey) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .get(key)
            .is_some_and(|request| request.state == ActiveRequestState::CancellationRequested)
    }

    pub(crate) fn active(&self, key: &RequestKey) -> Option<ActiveRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .get(key)
            .cloned()
    }

    pub(crate) fn remove(&self, key: &RequestKey) -> Option<ActiveRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .remove(key)
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, key: &RequestKey) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .contains_key(key)
    }

    pub(crate) fn owned_by(&self, session_id: &McpSessionId) -> Vec<ActiveRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .values()
            .filter(|request| &request.key.session_id == session_id)
            .cloned()
            .collect()
    }

    pub(crate) fn all(&self) -> Vec<ActiveRequest> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .values()
            .cloned()
            .collect()
    }

    pub(crate) fn record_error(&self, key: RequestKey, error: OperationError) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .errors
            .push_back((key.clone(), error.for_request(key)));
        while state.errors.len() > MAX_RETAINED_REQUEST_ERRORS {
            state.errors.pop_front();
        }
    }

    #[cfg(test)]
    pub(crate) fn latest_error(&self, key: &RequestKey) -> Option<OperationError> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .errors
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, error)| error.clone())
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
        let active = registry.request_cancellation(&key).unwrap();
        assert!(matches!(
            active.cancellation,
            RequestCancellationTarget::Runtime(RpcRequestId::Number(101))
        ));
    }

    #[test]
    fn cancellation_intent_is_owned_by_the_scoped_request() {
        let registry = RequestRegistry::default();
        let request = key("a", 3);
        registry
            .register(request.clone(), runtime_target(103))
            .unwrap();

        assert!(!registry.cancellation_was_requested(&request));
        assert_eq!(
            registry.request_cancellation(&request).unwrap().state,
            ActiveRequestState::CancellationRequested
        );
        assert!(registry.cancellation_was_requested(&request));
    }

    #[test]
    fn queued_cancellation_intent_survives_runtime_target_activation() {
        let registry = RequestRegistry::default();
        let request = key("a", 3);
        registry
            .register(request.clone(), RequestCancellationTarget::QueuedWork)
            .unwrap();
        registry.request_cancellation(&request).unwrap();

        let activated = registry
            .replace_cancellation_target(&request, runtime_target(103))
            .unwrap();
        assert_eq!(activated.state, ActiveRequestState::CancellationRequested);
        assert!(matches!(
            activated.cancellation,
            RequestCancellationTarget::Runtime(RpcRequestId::Number(103))
        ));
    }

    #[test]
    fn reused_request_key_does_not_inherit_terminal_request_cancellation() {
        let registry = RequestRegistry::default();
        let request = key("a", 3);
        registry
            .register(request.clone(), runtime_target(103))
            .unwrap();
        registry.request_cancellation(&request).unwrap();
        registry.remove(&request).unwrap();
        registry
            .register(request.clone(), runtime_target(104))
            .unwrap();

        let active = registry.active(&request).unwrap();
        assert_eq!(active.state, ActiveRequestState::Active);
        assert!(matches!(
            active.cancellation,
            RequestCancellationTarget::Runtime(RpcRequestId::Number(104))
        ));
    }

    #[test]
    fn request_error_history_keeps_the_scoped_request_key() {
        let registry = RequestRegistry::default();
        let request = key("a", 4);
        registry.record_error(
            request.clone(),
            OperationError::new(
                "Request.Unavailable",
                crate::domain::ErrorCategory::Unavailable,
                "unavailable",
                true,
            ),
        );
        assert_eq!(
            registry.latest_error(&request).unwrap().request,
            Some(request)
        );
    }
}
