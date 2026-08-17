use std::fs;
use std::path::{Path, PathBuf};
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::path_authority::{PathAuthority, PathAuthorityError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct VerificationStep {
    pub priority: u8,
    pub kind: String,
    pub command: String,
    pub shell: String,
    pub source: String,
    pub evidence: String,
}

#[derive(Debug, Clone)]
pub(crate) struct VerificationPlanner {
    project_root: PathBuf,
    instruction_paths: Vec<PathBuf>,
}

impl VerificationPlanner {
    pub(crate) fn new(workspace: &Path, project_path: &str) -> Result<Self, PathAuthorityError> {
        let authority = PathAuthority::active_workspace(workspace)?;
        let project_root = authority.resolve_existing(project_path)?;
        if !project_root.is_dir() { return Err(PathAuthorityError::InvalidPath); }
        let mut instruction_paths = Vec::new();
        let mut cursor = Some(project_root.as_path());
        while let Some(directory) = cursor {
            for relative in ["AGENTS.md", ".github/copilot-instructions.md", "START_HERE.md"] {
                let candidate = directory.join(relative);
                if candidate.is_file() && authority.allows_canonical(&fs::canonicalize(&candidate).unwrap_or(candidate.clone())) {
                    instruction_paths.push(candidate);
                }
            }
            if authority.discovery_stops_at(directory) { break; }
            cursor = directory.parent();
        }
        instruction_paths.sort();
        instruction_paths.dedup();
        Ok(Self { project_root, instruction_paths })
    }

    pub(crate) fn plan(&self) -> Vec<VerificationStep> {
        let mut steps = Vec::new();
        self.instruction_steps(&mut steps);
        self.package_json_steps(&mut steps);
        self.cargo_steps(&mut steps);
        if self.git_root_present() {
            steps.push(step(50, "git_diff_checks", "git diff --check", "git", "project Git repository"));
        }
        let mut dedup = BTreeMap::<String, VerificationStep>::new();
        for candidate in steps {
            match dedup.get(&candidate.command) {
                Some(existing) if existing.priority <= candidate.priority => {}
                _ => { dedup.insert(candidate.command.clone(), candidate); }
            }
        }
        let mut steps = dedup.into_values().collect::<Vec<_>>();
        steps.sort_by(|left, right| left.priority.cmp(&right.priority).then_with(|| left.command.cmp(&right.command)));
        steps
    }

    fn instruction_steps(&self, steps: &mut Vec<VerificationStep>) {
        for path in &self.instruction_paths {
            let Ok(text) = fs::read_to_string(path) else { continue };
            for command in instruction_commands(&text) {
                steps.push(step(
                    10,
                    "project_instruction",
                    &command,
                    "project_instruction",
                    &format!("explicit command in {}", path.display()),
                ));
            }
        }
    }

    fn package_json_steps(&self, steps: &mut Vec<VerificationStep>) {
        let path = self.project_root.join("package.json");
        let Ok(text) = fs::read_to_string(&path) else { return };
        let Ok(document) = serde_json::from_str::<Value>(&text) else { return };
        let Some(scripts) = document.get("scripts").and_then(Value::as_object) else { return };
        for name in ["test:changed", "test:targeted"] {
            if scripts.get(name).and_then(Value::as_str).is_some() {
                steps.push(step(20, "changed_file_targeted", &format!("npm run {name}"), "package_script", &format!("package.json scripts.{name}")));
            }
        }
        if scripts.get("test").and_then(Value::as_str).is_some() {
            steps.push(step(30, "project_gate", "npm test", "package_script", "package.json scripts.test"));
        }
        if scripts.get("build").and_then(Value::as_str).is_some() {
            steps.push(step(30, "project_gate", "npm run build", "package_script", "package.json scripts.build"));
        }
        for name in ["lint", "typecheck"] {
            if scripts.get(name).and_then(Value::as_str).is_some() {
                steps.push(step(40, "lint_typecheck", &format!("npm run {name}"), "package_script", &format!("package.json scripts.{name}")));
            }
        }
    }

    fn cargo_steps(&self, steps: &mut Vec<VerificationStep>) {
        if !self.project_root.join("Cargo.toml").is_file() { return; }
        let locked = self.project_root.join("Cargo.lock").is_file();
        let suffix = if locked { " --locked" } else { "" };
        steps.push(step(30, "project_gate", &format!("cargo test{suffix}"), "cargo_manifest", "Cargo.toml manifest"));
        steps.push(step(40, "lint_typecheck", &format!("cargo clippy{suffix} --all-targets --all-features -- -D warnings"), "cargo_manifest", "Cargo.toml manifest"));
    }

    fn git_root_present(&self) -> bool {
        let mut cursor = Some(self.project_root.as_path());
        while let Some(directory) = cursor {
            if directory.join(".git").exists() { return true; }
            cursor = directory.parent();
        }
        false
    }
}

fn step(priority: u8, kind: &str, command: &str, source: &str, evidence: &str) -> VerificationStep {
    VerificationStep {
        priority,
        kind: kind.into(),
        command: command.into(),
        shell: "cmd".into(),
        source: source.into(),
        evidence: evidence.into(),
    }
}

fn instruction_commands(text: &str) -> Vec<String> {
    let mut commands = Vec::new();
    for (index, segment) in text.split('`').enumerate() {
        if index % 2 == 0 { continue; }
        let candidate = segment.trim();
        if candidate.contains('\n') || candidate.contains('\r') || candidate.len() > 512 { continue; }
        if is_supported_project_command(candidate) {
            commands.push(candidate.to_string());
        }
    }
    commands.sort();
    commands.dedup();
    commands
}

fn is_supported_project_command(command: &str) -> bool {
    [
        "npm test", "npm run ", "pnpm test", "pnpm run ", "yarn test", "yarn run ",
        "cargo test", "cargo clippy", "cargo fmt", "python -m pytest", "pytest ",
        "go test", "dotnet test", "mvn test", "mvn verify", "gradle test", "gradlew test",
        "git diff --check",
    ]
    .iter()
    .any(|prefix| command == prefix.trim_end() || command.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_uses_only_discovered_manifest_or_script_evidence() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let root = std::env::temp_dir().join(format!("localbridge-plan-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("package.json"), r#"{"scripts":{"test":"vitest run","lint":"eslint ."}}"#).unwrap();
        let planner = VerificationPlanner::new(&root, ".").unwrap();
        let plan = planner.plan();
        assert!(plan.iter().any(|step| step.command == "npm run lint" && step.evidence.contains("scripts.lint")));
        assert!(plan.iter().any(|step| step.command == "npm test" && step.evidence.contains("scripts.test")));
        assert!(plan.windows(2).all(|pair| pair[0].priority <= pair[1].priority));
        assert!(!plan.iter().any(|step| step.command.contains("cargo") || step.command.contains("pytest")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_instruction_commands_override_manifest_duplicates_and_targeted_scripts_precede_full_gate() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let root = std::env::temp_dir().join(format!("localbridge-plan-priority-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("AGENTS.md"), "Run `npm test` before completion. Do not execute `reg query HKCU`.").unwrap();
        fs::write(root.join("package.json"), r#"{"scripts":{"test":"vitest run","test:changed":"vitest related --run","lint":"eslint ."}}"#).unwrap();
        let planner = VerificationPlanner::new(&root, ".").unwrap();
        let plan = planner.plan();
        let npm_test = plan.iter().find(|step| step.command == "npm test").unwrap();
        assert_eq!(npm_test.priority, 10);
        assert_eq!(npm_test.source, "project_instruction");
        let targeted = plan.iter().find(|step| step.command == "npm run test:changed").unwrap();
        assert_eq!(targeted.priority, 20);
        assert!(!plan.iter().any(|step| step.command.starts_with("reg ")));
        let _ = fs::remove_dir_all(root);
    }
}
