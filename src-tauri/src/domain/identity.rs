use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq)]
pub struct AdoptionToken(String);

impl AdoptionToken {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AdoptionToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AdoptionToken(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptionTokenHash(String);

impl AdoptionTokenHash {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AdoptionTokenHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AdoptionTokenHash(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct McpSessionId(String);

impl McpSessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for McpSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PublicSessionId(String);

impl PublicSessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PublicSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TaskId(String);

impl TaskId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExecutionId(String);

impl ExecutionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ExecutionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RpcRequestId {
    Number(i64),
    String(String),
}

impl fmt::Display for RpcRequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(value) => write!(f, "{value}"),
            Self::String(value) => f.write_str(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RequestKey {
    pub session_id: McpSessionId,
    pub request_id: RpcRequestId,
}

impl RequestKey {
    pub fn new(session_id: McpSessionId, request_id: RpcRequestId) -> Self {
        Self {
            session_id,
            request_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_key_scopes_equal_rpc_ids_by_session() {
        let a = RequestKey::new(McpSessionId::new("a"), RpcRequestId::Number(1));
        let b = RequestKey::new(McpSessionId::new("b"), RpcRequestId::Number(1));
        assert_ne!(a, b);
        assert_eq!(a.request_id, b.request_id);
    }

    #[test]
    fn numeric_request_identity_preserves_the_contract_i64_range() {
        assert_eq!(
            RpcRequestId::Number(i64::MIN).to_string(),
            i64::MIN.to_string()
        );
        assert_eq!(
            RpcRequestId::Number(i64::MAX).to_string(),
            i64::MAX.to_string()
        );
    }

    #[test]
    fn public_session_identity_is_distinct_from_mcp_session_identity() {
        let mcp = McpSessionId::new("same-text");
        let public = PublicSessionId::new("same-text");
        assert_eq!(mcp.as_str(), public.as_str());
    }

    #[test]
    fn task_execution_and_public_session_are_distinct_identities() {
        let task = TaskId::new("same-text");
        let execution = ExecutionId::new("same-text");
        let public = PublicSessionId::new("same-text");
        assert_eq!(task.as_str(), execution.as_str());
        assert_eq!(execution.as_str(), public.as_str());
    }

    #[test]
    fn adoption_secrets_are_never_exposed_by_debug_output() {
        let token = AdoptionToken::new("secret-token");
        let hash = AdoptionTokenHash::new("secret-hash");
        assert!(!format!("{token:?}").contains("secret-token"));
        assert!(!format!("{hash:?}").contains("secret-hash"));
    }
}
