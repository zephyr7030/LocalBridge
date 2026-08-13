use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use super::model::{
    AppData, CURRENT_SETTINGS_SCHEMA_VERSION, StoredPermissionMode, StoredSettings,
};
use crate::workspace::{
    PendingWorkspaceConfirmation, PendingWorkspaceReason, WorkspaceEntry, WorkspaceId,
    WorkspacePersistence,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    pub data: AppData,
    pub original_version: u32,
    pub migrated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    InvalidJson(String),
    SchemaVersionMissing,
    SchemaVersionInvalid,
    ConfigurationVersionUnsupported { found: u32, current: u32 },
    HistoricalSchemaInvalid { version: u32, detail: String },
    CurrentSchemaInvalid(String),
    MigrationSerialization(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWorkspaceRecord {
    #[serde(default)]
    workspace_id: Option<String>,
    display_path: PathBuf,
    #[serde(default)]
    validated_identity: Option<String>,
    #[serde(default)]
    last_opened_at: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V1 {
    schema_version: u32,
    permission_mode: super::model::StoredPermissionMode,
    auto_start_services: bool,
    onboarding_complete: bool,
    #[serde(default)]
    workspace: Option<LegacyWorkspaceRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct V2 {
    schema_version: u32,
    settings: HistoricalStoredSettings,
    #[serde(default)]
    workspace: Option<LegacyWorkspaceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalStoredSettings {
    permission_mode: StoredPermissionMode,
    auto_start_services: bool,
    onboarding_complete: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct V3 {
    schema_version: u32,
    settings: HistoricalStoredSettings,
    workspace: WorkspacePersistence,
}

pub fn migrate_bytes(bytes: &[u8]) -> Result<MigrationOutcome, MigrationError> {
    let mut value: Value = serde_json::from_slice(bytes)
        .map_err(|error| MigrationError::InvalidJson(error.to_string()))?;
    let original_version = read_schema_version(&value)?;
    if original_version > CURRENT_SETTINGS_SCHEMA_VERSION {
        return Err(MigrationError::ConfigurationVersionUnsupported {
            found: original_version,
            current: CURRENT_SETTINGS_SCHEMA_VERSION,
        });
    }

    let mut version = original_version;
    while version < CURRENT_SETTINGS_SCHEMA_VERSION {
        value = match version {
            1 => migrate_v1_to_v2(value)?,
            2 => migrate_v2_to_v3(value)?,
            3 => migrate_v3_to_v4(value)?,
            other => {
                return Err(MigrationError::HistoricalSchemaInvalid {
                    version: other,
                    detail: "no sequential migration step registered".to_owned(),
                });
            }
        };
        let next = read_schema_version(&value)?;
        if next != version + 1 {
            return Err(MigrationError::HistoricalSchemaInvalid {
                version,
                detail: format!("migration must advance exactly one version, got {next}"),
            });
        }
        version = next;
    }

    let data: AppData = serde_json::from_value(value)
        .map_err(|error| MigrationError::CurrentSchemaInvalid(error.to_string()))?;
    data.validate()
        .map_err(|error| MigrationError::CurrentSchemaInvalid(format!("{error:?}")))?;
    Ok(MigrationOutcome {
        data,
        original_version,
        migrated: original_version != CURRENT_SETTINGS_SCHEMA_VERSION,
    })
}

fn read_schema_version(value: &Value) -> Result<u32, MigrationError> {
    let raw = value
        .get("schema_version")
        .ok_or(MigrationError::SchemaVersionMissing)?
        .as_u64()
        .ok_or(MigrationError::SchemaVersionInvalid)?;
    u32::try_from(raw).map_err(|_| MigrationError::SchemaVersionInvalid)
}

fn migrate_v1_to_v2(value: Value) -> Result<Value, MigrationError> {
    let old: V1 =
        serde_json::from_value(value).map_err(|error| MigrationError::HistoricalSchemaInvalid {
            version: 1,
            detail: error.to_string(),
        })?;
    if old.schema_version != 1 {
        return Err(MigrationError::HistoricalSchemaInvalid {
            version: 1,
            detail: "version tag mismatch".to_owned(),
        });
    }
    serde_json::to_value(V2 {
        schema_version: 2,
        settings: HistoricalStoredSettings {
            permission_mode: old.permission_mode,
            auto_start_services: old.auto_start_services,
            onboarding_complete: old.onboarding_complete,
        },
        workspace: old.workspace,
    })
    .map_err(|error| MigrationError::MigrationSerialization(error.to_string()))
}

fn migrate_v2_to_v3(value: Value) -> Result<Value, MigrationError> {
    let old: V2 =
        serde_json::from_value(value).map_err(|error| MigrationError::HistoricalSchemaInvalid {
            version: 2,
            detail: error.to_string(),
        })?;
    if old.schema_version != 2 {
        return Err(MigrationError::HistoricalSchemaInvalid {
            version: 2,
            detail: "version tag mismatch".to_owned(),
        });
    }

    let mut workspace = WorkspacePersistence::default();
    if let Some(old_workspace) = old.workspace {
        if old_workspace.display_path.as_os_str().is_empty() {
            return Err(MigrationError::HistoricalSchemaInvalid {
                version: 2,
                detail: "legacy workspace display path is empty".to_owned(),
            });
        }
        let id = old_workspace
            .workspace_id
            .as_deref()
            .filter(|value| !value.trim().is_empty());
        let identity = old_workspace
            .validated_identity
            .as_deref()
            .filter(|value| !value.trim().is_empty());
        match (id, identity) {
            (Some(id), Some(identity)) => {
                let entry = WorkspaceEntry::from_persisted_claim(
                    WorkspaceId::from_validated(id.to_owned()).map_err(|error| {
                        MigrationError::HistoricalSchemaInvalid {
                            version: 2,
                            detail: format!("invalid workspace id: {error:?}"),
                        }
                    })?,
                    old_workspace.display_path,
                    identity.to_owned(),
                    old_workspace.last_opened_at,
                )
                .map_err(|error| MigrationError::HistoricalSchemaInvalid {
                    version: 2,
                    detail: format!("invalid workspace entry: {error:?}"),
                })?;
                let active_id =
                    workspace
                        .registry
                        .upsert_persisted_claim(entry)
                        .map_err(|error| MigrationError::HistoricalSchemaInvalid {
                            version: 2,
                            detail: format!("registry migration failed: {error:?}"),
                        })?;
                workspace.active_workspace_id = Some(active_id);
            }
            (id, identity) => {
                workspace.pending_workspace_confirmation = Some(PendingWorkspaceConfirmation {
                    workspace_id: id.map(str::to_owned),
                    display_path: old_workspace.display_path,
                    reason: if identity.is_none() {
                        PendingWorkspaceReason::ValidatedIdentityMissing
                    } else {
                        PendingWorkspaceReason::WorkspaceIdMissing
                    },
                });
            }
        }
    }

    let current = V3 {
        schema_version: 3,
        settings: old.settings,
        workspace,
    };
    serde_json::to_value(current)
        .map_err(|error| MigrationError::MigrationSerialization(error.to_string()))
}

fn migrate_v3_to_v4(value: Value) -> Result<Value, MigrationError> {
    let old: V3 =
        serde_json::from_value(value).map_err(|error| MigrationError::HistoricalSchemaInvalid {
            version: 3,
            detail: error.to_string(),
        })?;
    if old.schema_version != 3 {
        return Err(MigrationError::HistoricalSchemaInvalid {
            version: 3,
            detail: "version tag mismatch".to_owned(),
        });
    }
    let current = AppData {
        schema_version: CURRENT_SETTINGS_SCHEMA_VERSION,
        settings: StoredSettings {
            permission_mode: old.settings.permission_mode,
            auto_start_services: old.settings.auto_start_services,
            close_window_continue_running: true,
            onboarding_complete: old.settings.onboarding_complete,
        },
        workspace: old.workspace,
    };
    current
        .validate()
        .map_err(|error| MigrationError::HistoricalSchemaInvalid {
            version: 3,
            detail: format!("migrated current data invalid: {error:?}"),
        })?;
    serde_json::to_value(current)
        .map_err(|error| MigrationError::MigrationSerialization(error.to_string()))
}
