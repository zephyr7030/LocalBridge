use localbridge_lib::state::ActiveWorkspaceState;
use localbridge_lib::workspace::{
    ValidatedWorkspaceIdentity, WorkspaceEntry, WorkspaceId, WorkspacePersistence,
};

fn entry(id: &str, path: &str, identity: &str, opened: u64) -> WorkspaceEntry {
    WorkspaceEntry::from_validator(
        WorkspaceId::from_validated(id).unwrap(),
        path,
        ValidatedWorkspaceIdentity::from_validator(identity).unwrap(),
        opened,
    )
    .unwrap()
}

#[test]
fn registry_deduplicates_by_validated_identity_not_display_path() {
    let mut state = WorkspacePersistence::default();
    let first_id = state
        .registry
        .upsert_validated(entry("id-1", r"D:\Project\One", "stable:volume:file-1", 1))
        .unwrap();
    let duplicate_id = state
        .registry
        .upsert_validated(entry("id-new", r"D:\Alias\One", "stable:volume:file-1", 2))
        .unwrap();
    assert_eq!(first_id, duplicate_id);
    assert_eq!(state.registry.entries().len(), 1);
    assert_eq!(state.registry.entries()[0].display_path.to_string_lossy(), r"D:\Alias\One");

    state
        .registry
        .upsert_validated(entry("id-2", r"D:\Alias\One", "stable:volume:file-2", 3))
        .unwrap();
    assert_eq!(state.registry.entries().len(), 2);
}

#[test]
fn remembered_registry_never_implies_multi_root_authorization() {
    let mut state = WorkspacePersistence::default();
    let first = state
        .registry
        .upsert_validated(entry("id-1", r"D:\One", "identity:1", 1))
        .unwrap();
    state
        .registry
        .upsert_validated(entry("id-2", r"D:\Two", "identity:2", 2))
        .unwrap();
    assert_eq!(state.remembered_entries().len(), 2);
    assert!(state.active_entry().is_none());
    state.set_active(first).unwrap();
    assert_eq!(state.active_entry().unwrap().workspace_id.as_str(), "id-1");
    assert!(matches!(state.to_control_state().unwrap().active(), ActiveWorkspaceState::Active(_)));
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
