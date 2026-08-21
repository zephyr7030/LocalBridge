#![cfg(windows)]

use localbridge_lib::workspace::{
    WorkspaceId, WorkspacePersistence, WorkspaceRegistryError, WorkspaceValidator,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(1);

struct TempWorkspace(PathBuf);

impl TempWorkspace {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "localbridge-lb003-workspace-{label}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn add_validated(
    state: &mut WorkspacePersistence,
    validator: &WorkspaceValidator,
    id: &str,
    path: PathBuf,
    opened: u64,
) -> WorkspaceId {
    let validated = validator.validate(&path).unwrap();
    state
        .registry
        .upsert_validated(
            WorkspaceId::from_validated(id).unwrap(),
            path,
            &validated,
            opened,
        )
        .unwrap()
}

#[test]
fn registry_deduplicates_by_validated_identity_not_display_path() {
    let validator = WorkspaceValidator;
    let one = TempWorkspace::new("dedupe-one");
    let two = TempWorkspace::new("dedupe-two");
    let alias = PathBuf::from(format!(r"\\?\{}", one.path().display()));
    assert_ne!(one.path(), &alias);

    let mut state = WorkspacePersistence::default();
    let first_id = add_validated(&mut state, &validator, "id-1", one.path().clone(), 1);
    let duplicate_id = add_validated(&mut state, &validator, "id-new", alias, 2);
    assert_eq!(first_id, duplicate_id);
    assert_eq!(state.registry.entries().len(), 1);

    add_validated(&mut state, &validator, "id-2", two.path().clone(), 3);
    assert_eq!(state.registry.entries().len(), 2);
}

#[test]
fn remembered_registry_never_implies_multi_root_authorization() {
    let validator = WorkspaceValidator;
    let one = TempWorkspace::new("remembered-one");
    let two = TempWorkspace::new("remembered-two");
    let first_identity = validator.validate(one.path()).unwrap();

    let mut state = WorkspacePersistence::default();
    let first = add_validated(&mut state, &validator, "id-1", one.path().clone(), 1);
    add_validated(&mut state, &validator, "id-2", two.path().clone(), 2);
    assert_eq!(state.remembered_entries().len(), 2);
    assert!(state.active_entry().is_none());

    state.set_active_reference(first).unwrap();
    let active = state.resolve_active(&validator).unwrap().unwrap();
    assert_eq!(
        active.validated.identity().as_str(),
        first_identity.identity().as_str()
    );
}

#[test]
fn legitimate_persisted_workspace_is_revalidated_on_restart() {
    let validator = WorkspaceValidator;
    let workspace = TempWorkspace::new("restart");
    let expected = validator.validate(workspace.path()).unwrap();
    let mut state = WorkspacePersistence::default();
    let id = add_validated(
        &mut state,
        &validator,
        "id-restart",
        workspace.path().clone(),
        1,
    );
    state.set_active_reference(id).unwrap();

    let json = serde_json::to_string(&state).unwrap();
    let decoded: WorkspacePersistence = serde_json::from_str(&json).unwrap();
    let active = decoded.resolve_active(&validator).unwrap().unwrap();
    assert_eq!(active.validated.identity().as_str(), expected.identity().as_str());
    assert_eq!(active.validated.execution_path(), expected.execution_path());
}

#[test]
fn verbatim_alias_is_identity_only_and_active_execution_path_is_ordinary() {
    let validator = WorkspaceValidator;
    let workspace = TempWorkspace::new("verbatim-execution-boundary");
    let ordinary = validator.validate(workspace.path()).unwrap();
    let alias = PathBuf::from(format!(r"\\?\{}", workspace.path().display()));
    let through_alias = validator.validate(&alias).unwrap();

    assert_eq!(ordinary.identity(), through_alias.identity());
    assert!(through_alias.resolved_path().to_string_lossy().starts_with(r"\\?\"));
    assert!(!through_alias.execution_path().to_string_lossy().starts_with(r"\\?\"));

    let mut state = WorkspacePersistence::default();
    let id = state
        .registry
        .upsert_validated(
            WorkspaceId::from_validated("verbatim-execution").unwrap(),
            through_alias.execution_path(),
            &through_alias,
            1,
        )
        .unwrap();
    state.set_active_reference(id).unwrap();
    let active = state.resolve_active(&validator).unwrap().unwrap();
    assert_eq!(
        active.validated.identity().as_str(),
        through_alias.identity().as_str()
    );
    assert_eq!(
        active.validated.execution_path(),
        through_alias.execution_path()
    );
    assert!(
        !active
            .validated
            .execution_path()
            .to_string_lossy()
            .starts_with(r"\\?\")
    );
}

#[test]
fn deserialized_workspace_identity_is_revalidated_before_activation() {
    let validator = WorkspaceValidator;
    let workspace = TempWorkspace::new("forged-identity");
    let mut state = WorkspacePersistence::default();
    let id = add_validated(
        &mut state,
        &validator,
        "id-forged",
        workspace.path().clone(),
        1,
    );
    state.set_active_reference(id).unwrap();

    let mut json = serde_json::to_value(&state).unwrap();
    json["registry"]["entries"][0]["validated_identity"] =
        serde_json::Value::String("attacker-forged-identity".to_owned());
    let decoded: WorkspacePersistence = serde_json::from_value(json).unwrap();

    assert!(matches!(
        decoded.resolve_active(&validator),
        Err(WorkspaceRegistryError::PersistedIdentityMismatch)
    ));
}

#[test]
fn persisted_display_path_substitution_cannot_authorize_another_directory() {
    let validator = WorkspaceValidator;
    let original = TempWorkspace::new("original");
    let substituted = TempWorkspace::new("substituted");
    let mut state = WorkspacePersistence::default();
    let id = add_validated(
        &mut state,
        &validator,
        "id-original",
        original.path().clone(),
        1,
    );
    state.set_active_reference(id).unwrap();

    let mut json = serde_json::to_value(&state).unwrap();
    json["registry"]["entries"][0]["display_path"] =
        serde_json::Value::String(substituted.path().to_string_lossy().into_owned());
    let decoded: WorkspacePersistence = serde_json::from_value(json).unwrap();

    assert!(matches!(
        decoded.resolve_active(&validator),
        Err(WorkspaceRegistryError::PersistedIdentityMismatch)
    ));
}

#[test]
fn missing_workspace_after_restart_cannot_become_active() {
    let validator = WorkspaceValidator;
    let workspace = TempWorkspace::new("deleted");
    let path = workspace.path().clone();
    let mut state = WorkspacePersistence::default();
    let id = add_validated(&mut state, &validator, "id-deleted", path.clone(), 1);
    state.set_active_reference(id).unwrap();
    let json = serde_json::to_string(&state).unwrap();
    fs::remove_dir_all(&path).unwrap();
    let decoded: WorkspacePersistence = serde_json::from_str(&json).unwrap();

    assert!(matches!(
        decoded.resolve_active(&validator),
        Err(WorkspaceRegistryError::WorkspaceValidationWindowsApi {
            operation: "CreateFileW",
            ..
        })
    ));
}

#[test]
fn no_active_workspace_is_a_normal_persistable_state() {
    let state = WorkspacePersistence::default();
    assert!(state.is_no_active_workspace());
    assert!(state.domain_is_no_active_workspace().unwrap());
    let json = serde_json::to_string(&state).unwrap();
    assert!(json.contains("\"active_workspace_id\":null"));
    let decoded: WorkspacePersistence = serde_json::from_str(&json).unwrap();
    assert!(decoded.is_no_active_workspace());
}
