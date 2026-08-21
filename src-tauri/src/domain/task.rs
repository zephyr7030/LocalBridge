use serde::{Deserialize, Serialize};

use super::{LifecycleState, McpSessionId, OperationError, RequestKey, TaskId};

const MAX_SAFE_SUMMARY_CHARS: usize = 160;
const SENSITIVE_MARKERS: &[&str] = &[
    "authorization:",
    "bearer ",
    "api_key",
    "api-key",
    "runtime api key",
    "credential",
    "session secret",
    "broker nonce",
    "nonce=",
    "sk-",
];
const SENSITIVE_KEYS: &[&str] = &[
    "token",
    "access_token",
    "access-token",
    "refresh_token",
    "refresh-token",
    "id_token",
    "id-token",
    "auth_token",
    "auth-token",
    "session_token",
    "session-token",
    "client_token",
    "client-token",
    "api_key",
    "api-key",
    "apikey",
    "runtime_api_key",
    "runtime-api-key",
    "password",
    "passwd",
    "passphrase",
    "pwd",
    "secret",
    "private_key",
    "private-key",
    "secret_key",
    "secret-key",
    "client_secret",
    "client-secret",
    "session_secret",
    "session-secret",
    "authorization",
    "nonce",
    "credential",
];

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn contains_sensitive_key_value(lower: &str) -> bool {
    for key in SENSITIVE_KEYS {
        let mut search_from = 0;
        while let Some(relative_index) = lower[search_from..].find(key) {
            let index = search_from + relative_index;
            let end = index + key.len();
            let before = lower[..index].chars().next_back();
            let after = lower[end..].chars().next();
            let bounded_before = before.is_none_or(|ch| !is_identifier_char(ch));
            let bounded_after = after.is_none_or(|ch| !is_identifier_char(ch));
            if bounded_before && bounded_after {
                let suffix = lower[end..].trim_start_matches(|ch: char| {
                    ch.is_ascii_whitespace() || ch == '\'' || ch == '"'
                });
                if suffix.starts_with('=')
                    || suffix.starts_with(':')
                    || lower[..index].ends_with("--")
                {
                    return true;
                }
            }
            search_from = end;
        }
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    ReadFile,
    SearchCode,
    ModifyFile,
    ExecuteCommand,
    GitOperation,
    Build,
    Test,
    ElevatedOperation,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum SafeTaskSummary {
    Omitted,
    Text(String),
}

impl SafeTaskSummary {
    pub fn from_untrusted(raw: &str) -> Self {
        let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            return Self::Omitted;
        }
        let lower = normalized.to_ascii_lowercase();
        if SENSITIVE_MARKERS
            .iter()
            .any(|marker| lower.contains(marker))
            || contains_sensitive_key_value(&lower)
        {
            return Self::Omitted;
        }
        let mut chars = normalized.chars();
        let text: String = chars.by_ref().take(MAX_SAFE_SUMMARY_CHARS).collect();
        if chars.next().is_some() {
            Self::Text(format!("{text}…"))
        } else {
            Self::Text(text)
        }
    }

    pub fn as_deref(&self) -> Option<&str> {
        match self {
            Self::Omitted => None,
            Self::Text(value) => Some(value.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: TaskId,
    pub owner_session: McpSessionId,
    pub request: RequestKey,
    pub kind: TaskKind,
    pub summary: SafeTaskSummary,
    pub lifecycle: LifecycleState,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub error: Option<OperationError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_omits_secret_like_input_and_bounds_safe_text() {
        assert_eq!(
            SafeTaskSummary::from_untrusted("Authorization: Bearer hidden-value"),
            SafeTaskSummary::Omitted
        );
        assert_eq!(
            SafeTaskSummary::from_untrusted("read src/tokenizer.rs"),
            SafeTaskSummary::Text("read src/tokenizer.rs".to_string())
        );
        let summary = SafeTaskSummary::from_untrusted(&"a".repeat(400));
        assert!(summary.as_deref().unwrap().chars().count() <= MAX_SAFE_SUMMARY_CHARS + 1);
    }
}
