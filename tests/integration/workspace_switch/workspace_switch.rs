use super::*;
use crate::runtime::RuntimeDriver;
use crate::state::{CurrentTaskStatus, RuntimeFault, RuntimeState};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(1);

type Events = Rc<RefCell<Vec<String>>>;
type Reject = Rc<RefCell<Option<PathBuf>>>;

struct TempDir(PathBuf);
impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("localbridge-lb010-{label}-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("keep.txt"), b"keep").unwrap();
        Self(path)
    }
    fn path(&self) -> &Path { &self.0 }
}
impl Drop for TempDir { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

#[derive(Clone)]
struct Driver {
    workspace: PathBuf,
    events: Events,
    reject: Reject,
}
impl Driver {
    fn new(workspace: &Path) -> (Self, Events, Reject) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let reject = Rc::new(RefCell::new(None));
        (Self { workspace: workspace.to_path_buf(), events: events.clone(), reject: reject.clone() }, events, reject)
    }
    fn log(&self, name: &str) { self.events.borrow_mut().push(format!("{name}:{}", self.workspace.display())); }
}
impl RuntimeDriver for Driver {
    type Mcp = PathBuf;
    type Pep = PathBuf;
    type Tunnel = PathBuf;
    fn start_mcp(&mut self) -> Result<Self::Mcp, RuntimeFault> {
        self.log("mcp.start");
        if self.reject.borrow().as_ref().is_some_and(|path| path == &self.workspace) { return Err(RuntimeFault::WorkspaceInvalid); }
        Ok(self.workspace.clone())
    }
    fn confirm_mcp_ready(&mut self, _mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> { self.log("mcp.ready"); Ok(()) }
    fn start_pep(&mut self, mcp: Self::Mcp) -> Result<Self::Pep, RuntimeFault> { self.log("pep.start"); Ok(mcp) }
    fn confirm_pep_ready(&mut self, _pep: &Self::Pep) -> Result<(), RuntimeFault> { self.log("pep.ready"); Ok(()) }
    fn start_tunnel(&mut self, pep: &Self::Pep) -> Result<Self::Tunnel, RuntimeFault> { self.log("tunnel.start"); Ok(pep.clone()) }
    fn confirm_tunnel_ready(&mut self, _tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> { self.log("tunnel.ready"); Ok(()) }
    fn stop_tunnel(&mut self, _tunnel: &mut Self::Tunnel) -> Result<(), RuntimeFault> { self.log("tunnel.stop"); Ok(()) }
    fn stop_pep(&mut self, pep: Self::Pep) -> Result<Self::Mcp, RuntimeFault> { self.log("pep.stop"); Ok(pep) }
    fn stop_mcp(&mut self, _mcp: &mut Self::Mcp) -> Result<(), RuntimeFault> { self.log("mcp.stop"); Ok(()) }
    fn current_task(&self, _pep: &Self::Pep) -> CurrentTaskStatus { CurrentTaskStatus::Idle }
    fn current_workspace(&self) -> Option<&Path> { Some(&self.workspace) }
    fn configure_workspace(&mut self, workspace: PathBuf) -> Result<(), RuntimeFault> { self.workspace = workspace; Ok(()) }
}

fn coordinator(label: &str) -> (TempDir, WorkspaceCoordinator) {
    let state = TempDir::new(label);
    let store = SettingsStore::new(state.path().join("settings.json"));
    let coordinator = WorkspaceCoordinator::load(store).unwrap();
    (state, coordinator)
}
fn id(value: &str) -> WorkspaceId { WorkspaceId::from_validated(value).unwrap() }

#[test]
fn add_validates_deduplicates_and_commits_active_only_after_runtime_ready() {
    let (_state, mut coordinator) = coordinator("add");
    let project = TempDir::new("project-add");
    let (driver, events, _) = Driver::new(project.path());
    let mut runtime = RuntimeOrchestrator::new(driver);
    let first = coordinator.add_and_select(&mut runtime, id("one"), project.path(), 1).unwrap();
    assert_eq!(runtime.state(), &RuntimeState::Ready);
    assert_eq!(coordinator.data().workspace.active_workspace_id.as_ref(), Some(&first));
    assert_eq!(events.borrow().last().unwrap().split(':').next().unwrap(), "tunnel.ready");

    let alias = PathBuf::from(format!(r"\\?\{}", project.path().display()));
    let duplicate = coordinator.add_and_select(&mut runtime, id("duplicate"), &alias, 2).unwrap();
    assert_eq!(duplicate, first);
    assert_eq!(coordinator.data().workspace.remembered_entries().len(), 1);
}

#[test]
fn failed_candidate_never_becomes_active_and_runtime_rolls_back_to_previous_workspace() {
    let (_state, mut coordinator) = coordinator("rollback");
    let old = TempDir::new("old");
    let candidate = TempDir::new("candidate");
    let (driver, _, reject) = Driver::new(old.path());
    let mut runtime = RuntimeOrchestrator::new(driver);
    let old_id = coordinator.add_and_select(&mut runtime, id("old"), old.path(), 1).unwrap();
    let candidate_id = coordinator.add_and_select(&mut runtime, id("candidate"), candidate.path(), 2).unwrap();
    coordinator.select(&mut runtime, &old_id, 3).unwrap();
    *reject.borrow_mut() = Some(WorkspaceValidator.validate(candidate.path()).unwrap().execution_path().to_path_buf());

    let error = coordinator.select(&mut runtime, &candidate_id, 4).unwrap_err();
    assert!(matches!(error, WorkspaceControlError::RuntimeSwitch(_)));
    assert_eq!(coordinator.data().workspace.active_workspace_id.as_ref(), Some(&old_id));
    assert_eq!(runtime.state(), &RuntimeState::Ready);
    let runtime_path = runtime.configured_workspace().unwrap();
    let old_resolved = WorkspaceValidator.validate(old.path()).unwrap();
    assert_eq!(runtime_path, old_resolved.execution_path());
}

#[test]
fn remove_non_active_changes_metadata_only_and_never_deletes_project_files() {
    let (_state, mut coordinator) = coordinator("remove-nonactive");
    let one = TempDir::new("remove-one");
    let two = TempDir::new("remove-two");
    let (driver, events, _) = Driver::new(one.path());
    let mut runtime = RuntimeOrchestrator::new(driver);
    let one_id = coordinator.add_and_select(&mut runtime, id("one"), one.path(), 1).unwrap();
    let two_id = coordinator.add_and_select(&mut runtime, id("two"), two.path(), 2).unwrap();
    coordinator.select(&mut runtime, &one_id, 3).unwrap();
    events.borrow_mut().clear();

    assert_eq!(coordinator.remove(&mut runtime, &two_id).unwrap(), WorkspaceRemoval::RemovedRemembered);
    assert!(events.borrow().is_empty());
    assert!(two.path().join("keep.txt").exists());
    assert_eq!(coordinator.data().workspace.active_workspace_id.as_ref(), Some(&one_id));
}

#[test]
fn remove_active_stops_tunnel_pep_mcp_then_enters_no_active_without_fallback() {
    let (_state, mut coordinator) = coordinator("remove-active");
    let active = TempDir::new("active");
    let remembered = TempDir::new("remembered");
    let (driver, events, _) = Driver::new(active.path());
    let mut runtime = RuntimeOrchestrator::new(driver);
    let active_id = coordinator.add_and_select(&mut runtime, id("active"), active.path(), 1).unwrap();
    let remembered_id = coordinator.add_and_select(&mut runtime, id("remembered"), remembered.path(), 2).unwrap();
    coordinator.select(&mut runtime, &active_id, 3).unwrap();
    events.borrow_mut().clear();

    assert_eq!(coordinator.remove(&mut runtime, &active_id).unwrap(), WorkspaceRemoval::RemovedActive);
    let stop_names = events.borrow().iter().map(|event| event.split(':').next().unwrap().to_owned()).collect::<Vec<_>>();
    assert_eq!(stop_names, ["tunnel.stop", "pep.stop", "mcp.stop"]);
    assert_eq!(runtime.state(), &RuntimeState::Stopped);
    assert!(coordinator.data().workspace.is_no_active_workspace());
    assert!(coordinator.data().workspace.registry.get(&remembered_id).is_some());
    assert!(active.path().join("keep.txt").exists());
}

#[test]
fn missing_active_workspace_fails_closed_and_does_not_authorize_remembered_project() {
    let (state, mut coordinator) = coordinator("missing-active");
    let active = TempDir::new("missing-current");
    let remembered = TempDir::new("still-valid");
    let (driver, _, _) = Driver::new(active.path());
    let mut runtime = RuntimeOrchestrator::new(driver);
    let active_id = coordinator.add_and_select(&mut runtime, id("active"), active.path(), 1).unwrap();
    let remembered_id = coordinator.add_and_select(&mut runtime, id("remembered"), remembered.path(), 2).unwrap();
    coordinator.select(&mut runtime, &active_id, 3).unwrap();
    runtime.stop().unwrap();
    fs::remove_dir_all(active.path()).unwrap();
    std::mem::forget(active);

    let reloaded = WorkspaceCoordinator::load(SettingsStore::new(state.path().join("settings.json"))).unwrap();
    assert!(reloaded.validate_active_state().is_err());
    assert_eq!(reloaded.data().workspace.active_workspace_id.as_ref(), Some(&active_id));
    assert!(reloaded.data().workspace.registry.get(&remembered_id).is_some());
}

#[test]
fn no_active_workspace_candidate_failure_never_restarts_stale_configured_driver_path() {
    let (_state, mut coordinator) = coordinator("no-active-stale");
    let stale = TempDir::new("stale-configured");
    let candidate = TempDir::new("rejected-candidate");
    let (driver, events, reject) = Driver::new(stale.path());
    let mut runtime = RuntimeOrchestrator::new(driver);
    assert_eq!(runtime.state(), &RuntimeState::Stopped);
    assert!(coordinator.data().workspace.is_no_active_workspace());
    let candidate_execution = WorkspaceValidator
        .validate(candidate.path())
        .unwrap()
        .execution_path()
        .to_path_buf();
    *reject.borrow_mut() = Some(candidate_execution);

    let error = coordinator
        .add_and_select(&mut runtime, id("candidate"), candidate.path(), 1)
        .unwrap_err();
    assert!(matches!(error, WorkspaceControlError::RuntimeSwitch(_)));
    assert!(coordinator.data().workspace.active_workspace_id.is_none());
    assert!(!runtime.state().is_ready());
    let started = events
        .borrow()
        .iter()
        .filter(|event| event.starts_with("mcp.start:"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(started.len(), 1, "stale configured path must not be restarted");
    assert!(started[0].contains("rejected-candidate"));
}
