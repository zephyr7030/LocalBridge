#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionMode {
    Edit,
    Full,
    Elevated,
}

impl PermissionMode {
    pub const fn ordinary_execution_mode(self) -> Self {
        match self {
            Self::Elevated => Self::Full,
            mode => mode,
        }
    }

    pub const fn elevation_selected(self) -> bool {
        matches!(self, Self::Elevated)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Read,
    Write,
    ProcessExec,
    Git,
    Workflow,
    PrivilegedExternalRuntime,
    ElevatedExec,
    ControlPlane,
    Unknown,
}

impl Capability {
    pub const fn is_control_plane(self) -> bool {
        matches!(self, Self::ControlPlane)
    }

    pub const fn requires_privileged_broker(self) -> bool {
        matches!(self, Self::ElevatedExec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevated_reuses_full_for_ordinary_execution_without_implying_broker_state() {
        assert_eq!(
            PermissionMode::Elevated.ordinary_execution_mode(),
            PermissionMode::Full
        );
        assert!(PermissionMode::Elevated.elevation_selected());
        assert!(!PermissionMode::Full.elevation_selected());
    }

    #[test]
    fn control_plane_and_elevated_exec_remain_distinct_capabilities() {
        assert!(Capability::ControlPlane.is_control_plane());
        assert!(!Capability::ElevatedExec.is_control_plane());
        assert!(Capability::ElevatedExec.requires_privileged_broker());
    }
}
