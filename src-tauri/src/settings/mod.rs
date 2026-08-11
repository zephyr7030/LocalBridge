mod migration;
mod model;
mod store;

pub use migration::{migrate_bytes, MigrationError, MigrationOutcome};
pub use model::{
    AppData, AppDataValidationError, StoredPermissionMode, StoredSettings,
    CURRENT_SETTINGS_SCHEMA_VERSION,
};
pub use store::{SettingsStore, SettingsStoreError};
