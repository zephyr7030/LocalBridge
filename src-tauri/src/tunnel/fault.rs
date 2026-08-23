use std::fmt;

use crate::credentials::CredentialStoreError;
use crate::diagnostics::error::{
    DiagnosticErrorCode, DiagnosticPhase, ErrorDiagnostic, transport_unavailable,
};
use crate::runtime::SupervisorError;
use crate::state::RuntimeFault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retryability {
    Recoverable,
    NonRecoverable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPlaneFault {
    BadRequest,
    Authentication,
    Authorization,
    TunnelNotFound,
    RateLimited,
    Server,
    Timeout,
    Tls,
    Network,
    Unknown,
}

impl ControlPlaneFault {
    pub const fn retryability(self) -> Retryability {
        match self {
            Self::RateLimited | Self::Server | Self::Timeout | Self::Network => {
                Retryability::Recoverable
            }
            Self::BadRequest
            | Self::Authentication
            | Self::Authorization
            | Self::TunnelNotFound
            | Self::Tls
            | Self::Unknown => Retryability::NonRecoverable,
        }
    }

    pub fn diagnostic(self) -> ErrorDiagnostic {
        match self {
            Self::BadRequest => transport_unavailable("http_400", Some(400)),
            Self::Authentication => ErrorDiagnostic::new(
                DiagnosticErrorCode::Denied,
                DiagnosticPhase::Transport,
                "http_401_authentication",
            )
            .with_http_status(401),
            Self::Authorization => ErrorDiagnostic::new(
                DiagnosticErrorCode::Denied,
                DiagnosticPhase::Transport,
                "http_403_authorization",
            )
            .with_http_status(403),
            Self::TunnelNotFound => ErrorDiagnostic::new(
                DiagnosticErrorCode::InvalidRequest,
                DiagnosticPhase::Transport,
                "http_404_tunnel_not_found",
            )
            .with_http_status(404),
            Self::RateLimited => transport_unavailable("http_429_rate_limited", Some(429)),
            Self::Server => transport_unavailable("control_plane_server", None),
            Self::Timeout => ErrorDiagnostic::new(
                DiagnosticErrorCode::Timeout,
                DiagnosticPhase::Transport,
                "control_plane_timeout",
            ),
            Self::Tls => transport_unavailable("tls_failure", None),
            Self::Network => transport_unavailable("network_failure", None),
            Self::Unknown => ErrorDiagnostic::new(
                DiagnosticErrorCode::Unknown,
                DiagnosticPhase::Transport,
                "control_plane_unknown",
            ),
        }
    }
}

#[derive(Debug)]
pub enum TunnelError {
    InvalidInstallRoot,
    InvalidTunnelId,
    InvalidMcpTarget,
    InvalidHealthStateDirectory,
    InvalidControlPlaneOverride,
    RuntimeMissing,
    RuntimeChecksumMismatch,
    RuntimeKeyMissing,
    SecretStoreFailed(CredentialStoreError),
    SecretInjectionUnsupported,
    HealthStateIo,
    HealthUrlInvalid,
    HealthUnavailable,
    TunnelSpawnFailed(SupervisorError),
    ProcessOwnershipFailed(SupervisorError),
    TunnelExited,
    HealthTimeout,
    HealthProtocol,
    ControlPlane(ControlPlaneFault),
    RestartDenied,
}

impl TunnelError {
    pub const fn retryability(&self) -> Retryability {
        match self {
            Self::TunnelExited | Self::HealthTimeout | Self::HealthUnavailable => {
                Retryability::Recoverable
            }
            Self::ControlPlane(fault) => fault.retryability(),
            _ => Retryability::NonRecoverable,
        }
    }

    pub const fn runtime_fault(&self) -> RuntimeFault {
        match self {
            Self::RuntimeMissing => RuntimeFault::RuntimeMissing,
            Self::RuntimeChecksumMismatch => RuntimeFault::RuntimeChecksumMismatch,
            Self::RuntimeKeyMissing => RuntimeFault::RuntimeKeyMissing,
            Self::SecretStoreFailed(_) => RuntimeFault::SecretStoreFailed,
            Self::SecretInjectionUnsupported => RuntimeFault::SecretInjectionUnsupported,
            Self::TunnelSpawnFailed(_) => RuntimeFault::TunnelSpawnFailed,
            Self::ProcessOwnershipFailed(_) => RuntimeFault::ProcessOwnershipFailed,
            Self::TunnelExited => RuntimeFault::TunnelExited,
            Self::HealthStateIo | Self::HealthTimeout | Self::HealthUnavailable => {
                RuntimeFault::TunnelHealthTimeout
            }
            Self::ControlPlane(
                ControlPlaneFault::Authentication | ControlPlaneFault::Authorization,
            ) => RuntimeFault::TunnelAuthFailed,
            Self::ControlPlane(
                ControlPlaneFault::RateLimited
                | ControlPlaneFault::Server
                | ControlPlaneFault::Timeout
                | ControlPlaneFault::Network,
            ) => RuntimeFault::TunnelHealthTimeout,
            Self::InvalidInstallRoot
            | Self::InvalidTunnelId
            | Self::InvalidMcpTarget
            | Self::InvalidHealthStateDirectory
            | Self::InvalidControlPlaneOverride
            | Self::HealthUrlInvalid
            | Self::HealthProtocol
            | Self::RestartDenied
            | Self::ControlPlane(
                ControlPlaneFault::BadRequest
                | ControlPlaneFault::TunnelNotFound
                | ControlPlaneFault::Tls
                | ControlPlaneFault::Unknown,
            ) => RuntimeFault::ConfigurationInvalid,
        }
    }

    pub fn diagnostic(&self) -> ErrorDiagnostic {
        match self {
            Self::ControlPlane(fault) => fault.diagnostic(),
            Self::HealthUnavailable => transport_unavailable("health_unavailable", None),
            Self::HealthTimeout => ErrorDiagnostic::new(
                DiagnosticErrorCode::Timeout,
                DiagnosticPhase::Transport,
                "health_timeout",
            ),
            Self::HealthProtocol => ErrorDiagnostic::new(
                DiagnosticErrorCode::Unavailable,
                DiagnosticPhase::Transport,
                "health_protocol",
            ),
            _ => ErrorDiagnostic::new(
                DiagnosticErrorCode::Unknown,
                DiagnosticPhase::Runtime,
                format!("tunnel_{:?}", self.runtime_fault()).to_ascii_lowercase(),
            ),
        }
    }
}

impl fmt::Display for TunnelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInstallRoot => f.write_str("invalid tunnel installation root"),
            Self::InvalidTunnelId => f.write_str("invalid Tunnel ID"),
            Self::InvalidMcpTarget => f.write_str("invalid MCP tunnel target"),
            Self::InvalidHealthStateDirectory => {
                f.write_str("invalid tunnel health state directory")
            }
            Self::InvalidControlPlaneOverride => f.write_str("invalid test control-plane override"),
            Self::RuntimeMissing => f.write_str("bundled tunnel runtime is missing"),
            Self::RuntimeChecksumMismatch => {
                f.write_str("bundled tunnel runtime integrity check failed")
            }
            Self::RuntimeKeyMissing => f.write_str("Runtime API Key is missing"),
            Self::SecretStoreFailed(error) => write!(f, "secure credential read failed: {error}"),
            Self::SecretInjectionUnsupported => {
                f.write_str("safe tunnel secret injection is unsupported")
            }
            Self::HealthStateIo => f.write_str("tunnel health state I/O failed"),
            Self::HealthUrlInvalid => f.write_str("tunnel health URL is not a valid loopback URL"),
            Self::HealthUnavailable => f.write_str("tunnel local health endpoint is unavailable"),
            Self::TunnelSpawnFailed(error) => write!(f, "tunnel process spawn failed: {error}"),
            Self::ProcessOwnershipFailed(error) => {
                write!(f, "tunnel process ownership failed: {error}")
            }
            Self::TunnelExited => f.write_str("tunnel process exited"),
            Self::HealthTimeout => f.write_str("tunnel readiness timed out"),
            Self::HealthProtocol => f.write_str("tunnel local health protocol is invalid"),
            Self::ControlPlane(fault) => write!(f, "tunnel control-plane fault: {fault:?}"),
            Self::RestartDenied => {
                f.write_str("non-recoverable tunnel fault cannot be automatically restarted")
            }
        }
    }
}

impl std::error::Error for TunnelError {}

pub fn classify_control_plane_error(message: &str) -> ControlPlaneFault {
    let lower = message.to_ascii_lowercase();
    if lower.contains("400") || lower.contains("bad request") {
        ControlPlaneFault::BadRequest
    } else if lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("invalid api key")
    {
        ControlPlaneFault::Authentication
    } else if lower.contains("403")
        || lower.contains("forbidden")
        || lower.contains("tunnel_use_forbidden")
    {
        ControlPlaneFault::Authorization
    } else if lower.contains("404")
        || lower.contains("tunnel_not_found")
        || lower.contains("not found")
    {
        ControlPlaneFault::TunnelNotFound
    } else if lower.contains("429") || lower.contains("rate limit") {
        ControlPlaneFault::RateLimited
    } else if lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("server error")
    {
        ControlPlaneFault::Server
    } else if lower.contains("408")
        || lower.contains("timeout")
        || lower.contains("deadline exceeded")
    {
        ControlPlaneFault::Timeout
    } else if lower.contains("x509") || lower.contains("certificate") || lower.contains("tls") {
        ControlPlaneFault::Tls
    } else if lower.contains("dial tcp")
        || lower.contains("connection refused")
        || lower.contains("actively refused")
        || lower.contains("no such host")
        || lower.contains("network")
    {
        ControlPlaneFault::Network
    } else {
        ControlPlaneFault::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_plane_faults_have_stable_typed_retryability() {
        for (message, expected, retryability) in [
            (
                "400 bad request",
                ControlPlaneFault::BadRequest,
                Retryability::NonRecoverable,
            ),
            (
                "401 unauthorized",
                ControlPlaneFault::Authentication,
                Retryability::NonRecoverable,
            ),
            (
                "403 tunnel_use_forbidden",
                ControlPlaneFault::Authorization,
                Retryability::NonRecoverable,
            ),
            (
                "404 tunnel_not_found",
                ControlPlaneFault::TunnelNotFound,
                Retryability::NonRecoverable,
            ),
            (
                "429 rate limit",
                ControlPlaneFault::RateLimited,
                Retryability::Recoverable,
            ),
            (
                "503 server error",
                ControlPlaneFault::Server,
                Retryability::Recoverable,
            ),
            (
                "deadline exceeded",
                ControlPlaneFault::Timeout,
                Retryability::Recoverable,
            ),
            (
                "x509 certificate error",
                ControlPlaneFault::Tls,
                Retryability::NonRecoverable,
            ),
            (
                "dial tcp connection refused",
                ControlPlaneFault::Network,
                Retryability::Recoverable,
            ),
            (
                "unclassified failure",
                ControlPlaneFault::Unknown,
                Retryability::NonRecoverable,
            ),
        ] {
            let fault = classify_control_plane_error(message);
            assert_eq!(fault, expected);
            assert_eq!(fault.retryability(), retryability);
        }
    }

    #[test]
    fn schema42_http_400_has_transport_diagnostic_without_retry_semantic_change() {
        let fault = classify_control_plane_error("HTTP 400 Bad Request");
        assert_eq!(fault, ControlPlaneFault::BadRequest);
        assert_eq!(fault.retryability(), Retryability::NonRecoverable);
        assert_eq!(
            TunnelError::ControlPlane(fault).runtime_fault(),
            RuntimeFault::ConfigurationInvalid
        );
        let diagnostic = fault.diagnostic();
        assert_eq!(diagnostic.error_code, DiagnosticErrorCode::Unavailable);
        assert_eq!(diagnostic.phase, DiagnosticPhase::Transport);
        assert_eq!(diagnostic.cause, "http_400");
        assert_eq!(diagnostic.http_status, Some(400));
    }

    #[test]
    fn health_protocol_is_non_recoverable_but_endpoint_unavailability_is_recoverable() {
        assert_eq!(
            TunnelError::HealthProtocol.retryability(),
            Retryability::NonRecoverable
        );
        assert_eq!(
            TunnelError::HealthUnavailable.retryability(),
            Retryability::Recoverable
        );
        assert_eq!(
            TunnelError::HealthUnavailable.runtime_fault(),
            RuntimeFault::TunnelHealthTimeout
        );
    }
}
