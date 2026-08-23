use serde::Serialize;

use crate::domain::{ErrorCategory, OperationError, RpcRequestId, TaskId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct UiError(Box<UiErrorFields>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiErrorFields {
    pub code: String,
    pub category: ErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub operation_id: Option<String>,
    pub session_id: Option<String>,
    pub request_id: Option<RpcRequestId>,
    pub task_id: Option<TaskId>,
}

pub type UiResult<T> = Result<T, UiError>;

impl UiError {
    pub fn internal(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self(Box::new(UiErrorFields {
            code: code.into(),
            category: ErrorCategory::Internal,
            message: message.into(),
            retryable: true,
            operation_id: None,
            session_id: None,
            request_id: None,
            task_id: None,
        }))
    }

    pub fn from_string(error: impl Into<Self>) -> Self {
        error.into()
    }
}

impl From<OperationError> for UiError {
    fn from(error: OperationError) -> Self {
        let (session_id, request_id) = error.request.map_or((None, None), |request| {
            (
                Some(request.session_id.to_string()),
                Some(request.request_id),
            )
        });
        Self(Box::new(UiErrorFields {
            code: error.code,
            category: error.category,
            message: error.message,
            retryable: error.retryable,
            operation_id: error.operation_id,
            session_id,
            request_id,
            task_id: error.task_id,
        }))
    }
}

impl std::ops::Deref for UiError {
    type Target = UiErrorFields;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<String> for UiError {
    fn from(message: String) -> Self {
        Self::internal("Ui.OperationFailed", message)
    }
}

impl From<&str> for UiError {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{McpSessionId, RequestKey};

    #[test]
    fn ui_error_preserves_session_scoped_request_identity() {
        let error = OperationError::new(
            "Request.Denied",
            ErrorCategory::Authorization,
            "denied",
            false,
        )
        .for_request(RequestKey::new(
            McpSessionId::new("session-a"),
            RpcRequestId::Number(4),
        ));
        let ui = UiError::from(error);
        assert_eq!(ui.session_id.as_deref(), Some("session-a"));
        assert_eq!(ui.request_id, Some(RpcRequestId::Number(4)));
    }

    #[test]
    fn ui_error_envelope_remains_small_without_changing_its_typed_payload() {
        assert!(std::mem::size_of::<UiError>() <= 2 * std::mem::size_of::<usize>());
        let error = UiError::internal("Ui.Test", "test");
        let json = serde_json::to_value(error).unwrap();
        assert_eq!(json["code"], "Ui.Test");
        assert_eq!(json["category"], "internal");
    }
}
