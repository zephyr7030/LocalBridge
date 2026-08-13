use serde::{Deserialize, Serialize};

use crate::state::{PermissionMode, Settings};
use crate::workspace::{WorkspacePersistence, WorkspaceRegistryError};

pub const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredPermissionMode {
    Edit,
    Full,
    Elevated,
}

impl From<PermissionMode> for StoredPermissionMode {
    fn from(value: PermissionMode) -> Self {
        match value {
            PermissionMode::Edit => Self::Edit,
            PermissionMode::Full => Self::Full,
            PermissionMode::Elevated => Self::Elevated,
        }
    }
}

impl From<StoredPermissionMode> for PermissionMode {
    fn from(value: StoredPermissionMode) -> Self {
        match value {
            StoredPermissionMode::Edit => Self::Edit,
            StoredPermissionMode::Full => Self::Full,
            StoredPermissionMode::Elevated => Self::Elevated,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredSettings {
    pub permission_mode: StoredPermissionMode,
    pub auto_start_services: bool,
    pub close_window_continue_running: bool,
    pub onboarding_complete: bool,
}

impl Default for StoredSettings {
    fn default() -> Self {
        Self::from_domain(&Settings::default())
    }
}

impl StoredSettings {
    pub fn from_domain(settings: &Settings) -> Self {
        Self {
            permission_mode: settings.permission_mode.into(),
            auto_start_services: settings.auto_start_services,
            close_window_continue_running: true,
            onboarding_complete: settings.onboarding_complete,
        }
    }

    pub fn to_domain(&self) -> Settings {
        Settings {
            permission_mode: self.permission_mode.into(),
            auto_start_services: self.auto_start_services,
            onboarding_complete: self.onboarding_complete,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppData {
    pub schema_version: u32,
    pub settings: StoredSettings,
    pub workspace: WorkspacePersistence,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SETTINGS_SCHEMA_VERSION,
            settings: StoredSettings::default(),
            workspace: WorkspacePersistence::default(),
        }
    }
}

impl AppData {
    pub fn validate(&self) -> Result<(), AppDataValidationError> {
        if self.schema_version != CURRENT_SETTINGS_SCHEMA_VERSION {
            return Err(AppDataValidationError::WrongSchemaVersion {
                found: self.schema_version,
                expected: CURRENT_SETTINGS_SCHEMA_VERSION,
            });
        }
        self.workspace
            .validate()
            .map_err(AppDataValidationError::Workspace)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppDataValidationError {
    WrongSchemaVersion { found: u32, expected: u32 },
    Workspace(WorkspaceRegistryError),
}
