mod migration;
mod model;
mod store;

pub use migration::{MigrationError, MigrationOutcome, migrate_bytes};
pub use model::{
    AppData, AppDataValidationError, CURRENT_SETTINGS_SCHEMA_VERSION, StoredPermissionMode,
    StoredSettings,
};
pub use store::{SettingsStore, SettingsStoreError};
