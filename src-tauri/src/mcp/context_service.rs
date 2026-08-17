use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::path_authority::{PathAuthority, PathAuthorityError};

const MAX_CANDIDATE_FILES: usize = 96;
const MAX_FILE_BYTES: u64 = 128 * 1024;
const MAX_RELEVANT_RANGES: usize = 12;
const MAX_RANGE_LINES: usize = 9;

#[derive(Debug, Clone)]
pub(crate) struct ContextService {
    authority: PathAuthority,
    project_root: PathBuf,
}

impl ContextService {
    pub(crate) fn new(workspace: &Path, project_path: &str) -> Result<Self, PathAuthorityError> {
        let authority = PathAuthority::active_workspace(workspace)?;
        let project_root = authority.resolve_existing(project_path)?;
        if !project_root.is_dir() {
            return Err(PathAuthorityError::InvalidPath);
        }
        Ok(Self { authority, project_root })
    }

    pub(crate) fn discovery_metadata(&self) -> Value {
        json!({
            "important_files": self.important_files(),
            "instructions": self.discover_instructions()
        })
    }

    pub(crate) fn prepare(&self, objective: &str) -> Value {
        let instructions = self.discover_instructions();
        let important_files = self.important_files();
        let related = self.select_related_files(objective);
        let (ranges, files_read) = self.read_relevant_ranges(objective, &instructions, &related);
        json!({
            "instructions": instructions,
            "important_files": important_files,
            "related_files": related,
            "relevant_ranges": ranges,
            "files_read": files_read
        })
    }

    pub(crate) fn discover_instructions(&self) -> Vec<String> {
        let mut result = Vec::new();
        let mut cursor = Some(self.project_root.as_path());
        while let Some(directory) = cursor {
            for name in ["AGENTS.md", ".github/copilot-instructions.md"] {
                let candidate = directory.join(name);
                if candidate.is_file() {
                    if let Ok(path) = self.authority.display_path(&candidate) {
                        if !result.contains(&path) {
                            result.push(path);
                        }
                    }
                }
            }
            if self.authority.discovery_stops_at(directory) {
                break;
            }
            cursor = directory.parent();
        }
        for name in ["START_HERE.md", "README.md"] {
            if let Some(root) = self.authority.canonical_root() {
                let candidate = root.join(name);
                if candidate.is_file() {
                    if let Ok(path) = self.authority.display_path(&candidate) {
                        if !result.contains(&path) {
                            result.push(path);
                        }
                    }
                }
            }
        }
        result.truncate(12);
        result
    }

    pub(crate) fn important_files(&self) -> Vec<String> {
        let names = [
            "package.json", "Cargo.toml", "pyproject.toml", "go.mod", "pom.xml",
            "build.gradle", "build.gradle.kts", "Makefile", "CMakeLists.txt", "tsconfig.json",
            "vite.config.ts", "vite.config.js", "README.md", "AGENTS.md", "START_HERE.md",
        ];
        let mut result = Vec::new();
        for name in names {
            let candidate = self.project_root.join(name);
            if candidate.is_file() {
                if let Ok(path) = self.authority.display_path(&candidate) {
                    result.push(path);
                }
            }
        }
        result.truncate(20);
        result
    }

    pub(crate) fn select_related_files(&self, objective: &str) -> Vec<String> {
        let mut scored = self.search_text(objective);
        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        scored.into_iter().take(16).map(|(_, path)| path).collect()
    }

    pub(crate) fn search_text(&self, query: &str) -> Vec<(usize, String)> {
        let tokens = objective_tokens(query);
        let mut scored = Vec::<(usize, String)>::new();
        for path in self.candidate_files() {
            let Ok(metadata) = fs::metadata(&path) else { continue };
            if metadata.len() > MAX_FILE_BYTES { continue; }
            let Ok(bytes) = fs::read(&path) else { continue };
            let Ok(text) = std::str::from_utf8(&bytes) else { continue };
            let lower = text.to_lowercase();
            let mut score = tokens.iter().filter(|token| lower.contains(token.as_str())).count();
            if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
                let lower_name = name.to_lowercase();
                score += tokens.iter().filter(|token| lower_name.contains(token.as_str())).count() * 3;
            }
            if score == 0 && !tokens.is_empty() { continue; }
            if let Ok(display) = self.authority.display_path(&path) {
                scored.push((score, display));
            }
        }
        scored.truncate(MAX_CANDIDATE_FILES);
        scored
    }

    fn read_relevant_ranges(
        &self,
        objective: &str,
        instructions: &[String],
        related: &[String],
    ) -> (Vec<Value>, Vec<Value>) {
        let tokens = objective_tokens(objective);
        let mut ordered = Vec::new();
        ordered.extend(instructions.iter().cloned());
        ordered.extend(related.iter().cloned());
        let mut seen = BTreeSet::new();
        ordered.retain(|path| seen.insert(path.clone()));

        let mut ranges = Vec::new();
        let mut metadata = Vec::new();
        for relative in ordered {
            if ranges.len() >= MAX_RELEVANT_RANGES { break; }
            let Ok(path) = self.authority.resolve_existing(&relative) else { continue };
            let Ok(bytes) = fs::read(&path) else { continue };
            if bytes.len() as u64 > MAX_FILE_BYTES { continue; }
            let Ok(text) = std::str::from_utf8(&bytes) else { continue };
            let lines = text.lines().collect::<Vec<_>>();
            if lines.is_empty() { continue; }
            let hit = if instructions.contains(&relative) {
                Some(0)
            } else {
                lines.iter().position(|line| {
                    let lower = line.to_lowercase();
                    tokens.is_empty() || tokens.iter().any(|token| lower.contains(token.as_str()))
                })
            };
            let Some(hit) = hit else { continue };
            let start = hit.saturating_sub(3);
            let end = (start + MAX_RANGE_LINES).min(lines.len());
            let snippet = lines[start..end].join("\n");
            let identity = sha256_hex(&bytes);
            ranges.push(json!({
                "path": relative,
                "start_line": start + 1,
                "end_line": end,
                "text": snippet
            }));
            metadata.push(json!({
                "path": relative,
                "start_line": start + 1,
                "end_line": end,
                "content_sha256": identity,
                "bytes": bytes.len()
            }));
        }
        (ranges, metadata)
    }

    fn candidate_files(&self) -> Vec<PathBuf> {
        let mut result = Vec::new();
        let mut stack = vec![(self.project_root.clone(), 0usize)];
        while let Some((directory, depth)) = stack.pop() {
            if result.len() >= MAX_CANDIDATE_FILES { break; }
            let Ok(entries) = fs::read_dir(directory) else { continue };
            for entry in entries.flatten() {
                if result.len() >= MAX_CANDIDATE_FILES { break; }
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if entry.file_type().ok().is_some_and(|kind| kind.is_symlink()) { continue; }
                if path.is_dir() {
                    if depth < 3 && !matches!(name.as_str(), ".git" | "node_modules" | "target" | "dist" | "build" | "runtime") {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                if source_like(&path) { result.push(path); }
            }
        }
        result
    }
}

fn objective_tokens(objective: &str) -> Vec<String> {
    let mut tokens = objective
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .map(str::trim)
        .filter(|token| token.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    tokens.sort();
    tokens.dedup();
    tokens.truncate(12);
    tokens
}

fn source_like(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(),
            "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "go" | "java" | "kt" | "kts" |
            "c" | "cc" | "cpp" | "h" | "hpp" | "cs" | "toml" | "json" | "yaml" | "yml" | "md" | "xml" | "gradle"))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objective_tokens_are_bounded_and_deterministic() {
        assert_eq!(objective_tokens("Fix Runtime runtime HEALTH foo"), vec!["fix", "foo", "health", "runtime"]);
    }
}
