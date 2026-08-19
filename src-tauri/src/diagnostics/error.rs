use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiagnosticErrorCode {
    InvalidRequest,
    Unavailable,
    Denied,
    Timeout,
    Cancelled,
    ExecutionFailed,
    Unknown,
}

impl DiagnosticErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "InvalidRequest",
            Self::Unavailable => "Unavailable",
            Self::Denied => "Denied",
            Self::Timeout => "Timeout",
            Self::Cancelled => "Cancelled",
            Self::ExecutionFailed => "ExecutionFailed",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticPhase {
    Transport,
    Mcp,
    Runtime,
    Policy,
    Tool,
    Process,
    Unknown,
}

impl DiagnosticPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Transport => "transport",
            Self::Mcp => "mcp",
            Self::Runtime => "runtime",
            Self::Policy => "policy",
            Self::Tool => "tool",
            Self::Process => "process",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDiagnostic {
    pub error_code: DiagnosticErrorCode,
    pub phase: DiagnosticPhase,
    pub cause: String,
    pub http_status: Option<u16>,
}

impl ErrorDiagnostic {
    pub fn new(error_code: DiagnosticErrorCode, phase: DiagnosticPhase, cause: impl Into<String>) -> Self {
        Self { error_code, phase, cause: cause.into(), http_status: None }
    }

    pub fn with_http_status(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
    }

    pub fn to_value(&self) -> Value {
        json!({
            "error_code": self.error_code.as_str(),
            "phase": self.phase.as_str(),
            "cause": self.cause,
            "http_status": self.http_status,
        })
    }
}

pub fn from_canonical_code(code: &str) -> ErrorDiagnostic {
    match code {
        "InvalidArgument" | "NotFound" | "InvalidShellSyntax" | "FileChanged" | "PatchConflict" | "AmbiguousMatch" =>
            ErrorDiagnostic::new(DiagnosticErrorCode::InvalidRequest, DiagnosticPhase::Tool, canonical_cause(code)),
        "WorkspaceDenied" | "CapabilityDenied" | "PolicyDenied" | "PrivilegedRouteUnavailable" | "ElevationRequired" =>
            ErrorDiagnostic::new(DiagnosticErrorCode::Denied, DiagnosticPhase::Policy, canonical_cause(code)),
        "ProcessTimedOut" => ErrorDiagnostic::new(DiagnosticErrorCode::Timeout, DiagnosticPhase::Process, "process_timed_out"),
        "ProcessCancelled" => ErrorDiagnostic::new(DiagnosticErrorCode::Cancelled, DiagnosticPhase::Process, "process_cancelled"),
        "ProcessFailed" => ErrorDiagnostic::new(DiagnosticErrorCode::ExecutionFailed, DiagnosticPhase::Process, "process_failed"),
        "SessionUnavailable" => ErrorDiagnostic::new(DiagnosticErrorCode::Unavailable, DiagnosticPhase::Process, "session_unavailable"),
        "RuntimeProtocolMismatch" => ErrorDiagnostic::new(DiagnosticErrorCode::Unavailable, DiagnosticPhase::Mcp, "protocol_mismatch"),
        "RuntimeUnavailable" | "CapabilityUnavailable" | "RuntimeCapabilityMismatch" =>
            ErrorDiagnostic::new(DiagnosticErrorCode::Unavailable, DiagnosticPhase::Runtime, canonical_cause(code)),
        "OutputTruncated" => ErrorDiagnostic::new(DiagnosticErrorCode::ExecutionFailed, DiagnosticPhase::Tool, "output_truncated"),
        "Internal" => ErrorDiagnostic::new(DiagnosticErrorCode::Unknown, DiagnosticPhase::Unknown, "internal"),
        other => ErrorDiagnostic::new(DiagnosticErrorCode::Unknown, DiagnosticPhase::Unknown, format!("unmapped_{}", other.to_ascii_lowercase())),
    }
}

pub fn transport_unavailable(cause: impl Into<String>, http_status: Option<u16>) -> ErrorDiagnostic {
    let mut diagnostic = ErrorDiagnostic::new(DiagnosticErrorCode::Unavailable, DiagnosticPhase::Transport, cause);
    diagnostic.http_status = http_status;
    diagnostic
}

pub fn mcp_invalid(cause: impl Into<String>) -> ErrorDiagnostic {
    ErrorDiagnostic::new(DiagnosticErrorCode::InvalidRequest, DiagnosticPhase::Mcp, cause)
}

pub fn mcp_unavailable(cause: impl Into<String>) -> ErrorDiagnostic {
    ErrorDiagnostic::new(DiagnosticErrorCode::Unavailable, DiagnosticPhase::Mcp, cause)
}

pub fn mcp_unknown(cause: impl Into<String>) -> ErrorDiagnostic {
    ErrorDiagnostic::new(DiagnosticErrorCode::Unknown, DiagnosticPhase::Mcp, cause)
}

fn canonical_cause(code: &str) -> String {
    let mut cause = String::with_capacity(code.len() + 4);
    for (index, ch) in code.chars().enumerate() {
        if ch.is_ascii_uppercase() && index > 0 {
            cause.push('_');
        }
        cause.push(ch.to_ascii_lowercase());
    }
    cause
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_mapping_has_unknown_fallback_and_policy_phase() {
        let denied = from_canonical_code("PolicyDenied");
        assert_eq!(denied.error_code, DiagnosticErrorCode::Denied);
        assert_eq!(denied.phase, DiagnosticPhase::Policy);
        let unknown = from_canonical_code("FutureError");
        assert_eq!(unknown.error_code, DiagnosticErrorCode::Unknown);
        assert_eq!(unknown.phase, DiagnosticPhase::Unknown);
    }
}
