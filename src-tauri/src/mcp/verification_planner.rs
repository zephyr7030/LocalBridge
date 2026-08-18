use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::git_adapter::changed_paths;
use super::path_authority::{PathAuthority, PathAuthorityError};

const MAX_MANIFESTS: usize = 24;
const MAX_MANIFEST_DEPTH: usize = 4;

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
    authority: PathAuthority,
    project_root: PathBuf,
    instruction_paths: Vec<PathBuf>,
    changed_files: Vec<String>,
}

impl VerificationPlanner {
    pub(crate) fn new(workspace: &Path, project_path: &str) -> Result<Self, PathAuthorityError> {
        let authority = PathAuthority::active_workspace(workspace)?;
        let project_root = authority.resolve_existing(project_path)?;
        if !project_root.is_dir() {
            return Err(PathAuthorityError::InvalidPath);
        }
        let mut instruction_paths = Vec::new();
        let mut cursor = Some(project_root.as_path());
        while let Some(directory) = cursor {
            for relative in ["AGENTS.md", ".github/copilot-instructions.md", "START_HERE.md"] {
                let candidate = directory.join(relative);
                if candidate.is_file()
                    && fs::canonicalize(&candidate)
                        .ok()
                        .is_some_and(|path| authority.allows_canonical(&path))
                {
                    instruction_paths.push(candidate);
                }
            }
            if authority.discovery_stops_at(directory) {
                break;
            }
            cursor = directory.parent();
        }
        instruction_paths.sort();
        instruction_paths.dedup();
        let changed_files = changed_paths(workspace, project_path).unwrap_or_default();
        Ok(Self {
            authority,
            project_root,
            instruction_paths,
            changed_files,
        })
    }

    pub(crate) fn plan(&self) -> Vec<VerificationStep> {
        let mut steps = Vec::new();
        self.instruction_steps(&mut steps);
        self.package_json_steps(&mut steps);
        self.cargo_steps(&mut steps);
        if self.git_root_present() {
            steps.push(step(
                50,
                "git_diff_checks",
                "git diff --check",
                "git",
                "project Git repository",
            ));
        }
        let mut dedup = BTreeMap::<String, VerificationStep>::new();
        for candidate in steps {
            match dedup.get(&candidate.command) {
                Some(existing) if existing.priority <= candidate.priority => {}
                _ => {
                    dedup.insert(candidate.command.clone(), candidate);
                }
            }
        }
        let mut steps = dedup.into_values().collect::<Vec<_>>();
        steps.sort_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                .then_with(|| left.command.cmp(&right.command))
        });
        steps
    }

    fn instruction_steps(&self, steps: &mut Vec<VerificationStep>) {
        for path in &self.instruction_paths {
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            for command in instruction_commands(&text) {
                steps.push(step(
                    10,
                    "project_instruction",
                    &command,
                    "project_instruction",
                    &format!("explicit positive command in {}", path.display()),
                ));
            }
        }
    }

    fn package_json_steps(&self, steps: &mut Vec<VerificationStep>) {
        for path in self.manifest_files("package.json") {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(document) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            let Some(scripts) = document.get("scripts").and_then(Value::as_object) else {
                continue;
            };
            let prefix = self.npm_prefix(&path);
            if !self.changed_files.is_empty() {
                for name in ["test:changed", "test:targeted"] {
                    if scripts.get(name).and_then(Value::as_str).is_some() {
                        steps.push(step(
                            20,
                            "changed_file_targeted",
                            &format!("{prefix} run {name}"),
                            "package_script",
                            &format!(
                                "package.json scripts.{name}; changed files: {}",
                                self.changed_evidence()
                            ),
                        ));
                    }
                }
            }
            for name in ["lint", "typecheck"] {
                if scripts.get(name).and_then(Value::as_str).is_some() {
                    steps.push(step(
                        30,
                        "lint_typecheck",
                        &format!("{prefix} run {name}"),
                        "package_script",
                        &format!("{} scripts.{name}", self.relative_display(&path)),
                    ));
                }
            }
            if scripts.get("test").and_then(Value::as_str).is_some() {
                steps.push(step(
                    40,
                    "project_gate",
                    &format!("{prefix} test"),
                    "package_script",
                    &format!("{} scripts.test", self.relative_display(&path)),
                ));
            }
            if scripts.get("build").and_then(Value::as_str).is_some() {
                steps.push(step(
                    40,
                    "project_gate",
                    &format!("{prefix} run build"),
                    "package_script",
                    &format!("{} scripts.build", self.relative_display(&path)),
                ));
            }
        }
    }

    fn cargo_steps(&self, steps: &mut Vec<VerificationStep>) {
        for manifest in self.manifest_files("Cargo.toml") {
            let locked = manifest
                .parent()
                .is_some_and(|parent| parent.join("Cargo.lock").is_file());
            let suffix = if locked { " --locked" } else { "" };
            let manifest_arg = if manifest == self.project_root.join("Cargo.toml") {
                String::new()
            } else {
                format!(
                    " --manifest-path {}",
                    quote_cmd_arg(&self.relative_display(&manifest))
                )
            };
            steps.push(step(
                30,
                "lint_typecheck",
                &format!(
                    "cargo clippy{manifest_arg} --all-targets --all-features{suffix} -- -D warnings"
                ),
                "cargo_manifest",
                &format!("{} manifest", self.relative_display(&manifest)),
            ));
            steps.push(step(
                40,
                "project_gate",
                &format!("cargo test{manifest_arg}{suffix}"),
                "cargo_manifest",
                &format!("{} manifest", self.relative_display(&manifest)),
            ));
        }
    }

    fn manifest_files(&self, file_name: &str) -> Vec<PathBuf> {
        let mut result = Vec::new();
        let mut stack = vec![(self.project_root.clone(), 0usize)];
        let mut seen = BTreeSet::<PathBuf>::new();
        while let Some((directory, depth)) = stack.pop() {
            if result.len() >= MAX_MANIFESTS {
                break;
            }
            let Ok(canonical_dir) = fs::canonicalize(&directory) else {
                continue;
            };
            if !self.authority.allows_canonical(&canonical_dir) || !seen.insert(canonical_dir) {
                continue;
            }
            let candidate = directory.join(file_name);
            if candidate.is_file()
                && fs::canonicalize(&candidate)
                    .ok()
                    .is_some_and(|path| self.authority.allows_canonical(&path))
            {
                result.push(candidate);
            }
            if depth >= MAX_MANIFEST_DEPTH {
                continue;
            }
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            let mut children = entries
                .flatten()
                .filter_map(|entry| {
                    let file_type = entry.file_type().ok()?;
                    if !file_type.is_dir() || file_type.is_symlink() {
                        return None;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if excluded_directory(&name) {
                        return None;
                    }
                    let path = entry.path();
                    let canonical = fs::canonicalize(&path).ok()?;
                    self.authority.allows_canonical(&canonical).then_some(path)
                })
                .collect::<Vec<_>>();
            children.sort();
            for child in children.into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }
        result.sort();
        result.dedup();
        result
    }

    fn npm_prefix(&self, manifest: &Path) -> String {
        let parent = manifest.parent().unwrap_or(&self.project_root);
        if parent == self.project_root {
            "npm".into()
        } else {
            let relative = parent
                .strip_prefix(&self.project_root)
                .unwrap_or(parent)
                .to_string_lossy()
                .replace('\\', "/");
            format!("npm --prefix {}", quote_cmd_arg(&relative))
        }
    }

    fn relative_display(&self, path: &Path) -> String {
        path.strip_prefix(&self.project_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn changed_evidence(&self) -> String {
        let mut paths = self.changed_files.iter().take(8).cloned().collect::<Vec<_>>();
        if self.changed_files.len() > paths.len() {
            paths.push(format!("+{} more", self.changed_files.len() - paths.len()));
        }
        paths.join(", ")
    }

    fn git_root_present(&self) -> bool {
        let mut cursor = Some(self.project_root.as_path());
        while let Some(directory) = cursor {
            if directory.join(".git").exists() {
                return true;
            }
            cursor = directory.parent();
        }
        false
    }
}

fn excluded_directory(name: &str) -> bool {
    matches!(name, ".git" | "node_modules" | "dist" | "build" | "runtime")
        || name == "target"
        || name.starts_with("target-")
}

fn quote_cmd_arg(value: &str) -> String {
    if value.contains([' ', '\t']) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

fn step(
    priority: u8,
    kind: &str,
    command: &str,
    source: &str,
    evidence: &str,
) -> VerificationStep {
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
    let segments = text.split('`').collect::<Vec<_>>();
    let mut commands = Vec::new();
    for index in (1..segments.len()).step_by(2) {
        let candidate = segments[index].trim();
        if candidate.contains('\n') || candidate.contains('\r') || candidate.len() > 512 {
            continue;
        }
        let prose = segments.get(index.saturating_sub(1)).copied().unwrap_or_default();
        if is_negated_instruction(prose) {
            continue;
        }
        if is_supported_project_command(candidate) {
            commands.push(candidate.to_string());
        }
    }
    commands.sort();
    commands.dedup();
    commands
}

fn is_negated_instruction(prose: &str) -> bool {
    let tail = prose
        .chars()
        .rev()
        .take(160)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>()
        .to_lowercase();
    [
        "do not run",
        "do not execute",
        "don't run",
        "don't execute",
        "must not run",
        "must not execute",
        "never run",
        "never execute",
        "禁止运行",
        "不要运行",
        "不得运行",
    ]
    .iter()
    .any(|marker| tail.trim_end().ends_with(marker))
}

fn is_supported_project_command(command: &str) -> bool {
    [
        "npm test",
        "npm run ",
        "pnpm test",
        "pnpm run ",
        "yarn test",
        "yarn run ",
        "cargo test",
        "cargo clippy",
        "cargo fmt",
        "python -m pytest",
        "pytest ",
        "go test",
        "dotnet test",
        "mvn test",
        "mvn verify",
        "gradle test",
        "gradlew test",
        "git diff --check",
    ]
    .iter()
    .any(|prefix| command == prefix.trim_end() || command.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn temp_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "localbridge-plan-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git").args(args).current_dir(root).status().unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn planner_uses_required_precedence_and_mixed_node_rust_manifests() {
        let root = temp_root("mixed");
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"test":"vitest run","build":"vite build","lint":"eslint ."}}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("src-tauri")).unwrap();
        fs::write(
            root.join("src-tauri/Cargo.toml"),
            "[package]\nname='mixed-fixture'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        fs::write(root.join("src-tauri/Cargo.lock"), "").unwrap();
        let planner = VerificationPlanner::new(&root, ".").unwrap();
        let plan = planner.plan();
        assert!(plan.iter().any(|step| step.command == "npm run lint" && step.priority == 30));
        assert!(plan.iter().any(|step| step.command == "npm test" && step.priority == 40));
        assert!(plan.iter().any(|step| step.command == "npm run build" && step.priority == 40));
        assert!(plan.iter().any(|step| step.command.contains("cargo clippy --manifest-path src-tauri/Cargo.toml") && step.priority == 30));
        assert!(plan.iter().any(|step| step.command.contains("cargo test --manifest-path src-tauri/Cargo.toml") && step.priority == 40));
        assert!(plan.windows(2).all(|pair| pair[0].priority <= pair[1].priority));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn changed_targeted_scripts_require_a_real_git_changed_set() {
        let root = temp_root("changed");
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"test":"vitest run","test:changed":"vitest related --run"}}"#,
        )
        .unwrap();
        fs::write(root.join("a.ts"), "export const value = 1;\n").unwrap();
        git(&root, &["init"]);
        git(&root, &["config", "user.email", "planner@example.invalid"]);
        git(&root, &["config", "user.name", "Planner Fixture"]);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "baseline"]);
        let clean = VerificationPlanner::new(&root, ".").unwrap().plan();
        assert!(!clean.iter().any(|step| step.kind == "changed_file_targeted"));
        fs::write(root.join("a.ts"), "export const value = 2;\n").unwrap();
        let changed = VerificationPlanner::new(&root, ".").unwrap().plan();
        assert!(changed.iter().any(|step| {
            step.command == "npm run test:changed"
                && step.kind == "changed_file_targeted"
                && step.evidence.contains("a.ts")
        }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn negative_instruction_code_spans_are_not_execution_requirements() {
        let root = temp_root("negation");
        fs::write(
            root.join("AGENTS.md"),
            "Do not run `npm test`. Run `npm run lint`. Never execute `cargo test`.\n",
        )
        .unwrap();
        let planner = VerificationPlanner::new(&root, ".").unwrap();
        let plan = planner.plan();
        assert!(plan.iter().any(|step| step.command == "npm run lint" && step.priority == 10));
        assert!(!plan.iter().any(|step| step.command == "npm test"));
        assert!(!plan.iter().any(|step| step.command == "cargo test"));
        let _ = fs::remove_dir_all(root);
    }
}
