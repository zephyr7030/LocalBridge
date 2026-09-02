use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::runtime::run_bounded_command;
use crate::workspace::WorkspaceValidator;

use super::path_authority::{PathAuthorityError, WorkspaceResolver};

const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
const DEFAULT_TEXT_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_LINES: usize = 2_000;

#[cfg(test)]
pub(crate) fn handle_git_tool(workspace: &Path, name: &str, arguments: &Value) -> Option<Value> {
    let authority = match crate::workspace::WorkspaceResolver::active_workspace(workspace) {
        Ok(authority) => authority,
        Err(error) => return Some(resolve_error(authority_error(error))),
    };
    handle_git_tool_with_authority(&authority, name, arguments)
}

pub(crate) fn handle_git_tool_with_authority(
    authority: &WorkspaceResolver,
    name: &str,
    arguments: &Value,
) -> Option<Value> {
    if !matches!(
        name,
        "git_status" | "git_diff" | "git_log" | "git_show" | "git_blame"
    ) {
        return None;
    }
    let Some(arguments) = arguments.as_object() else {
        return Some(tool_error("INVALID_ARGUMENT", "Git 工具参数必须是对象"));
    };
    let resolver = GitRepositoryResolver::from_authority(authority.clone());
    match name {
        "git_status" => git_status(&resolver, arguments),
        "git_diff" => git_diff(&resolver, arguments),
        "git_log" => git_log(&resolver, arguments),
        "git_show" => git_show(&resolver, arguments),
        "git_blame" => git_blame(&resolver, arguments),
        _ => None,
    }
}

pub(crate) fn changed_paths_with_authority(
    authority: &WorkspaceResolver,
    path: &str,
) -> Result<Vec<String>, String> {
    let result = handle_git_tool_with_authority(
        authority,
        "git_status",
        &json!({"path":path,"include_untracked":true,"max_entries":10_000}),
    )
    .ok_or_else(|| "git status unavailable".to_string())?;
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err("git status failed".into());
    }
    let mut paths = result
        .pointer("/structuredContent/entries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("path").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolveError {
    InvalidPath,
    NotFound,
    OutsideWorkspace,
    ReparseEscape,
    RepositoryMismatch,
    InvalidRepository,
}

#[derive(Debug, Clone)]
struct ResolvedLocation {
    canonical: PathBuf,
    discovery_dir: PathBuf,
    is_dir: bool,
    display: String,
}

#[derive(Debug, Clone)]
struct GitRepository {
    canonical_root: PathBuf,
    execution_root: PathBuf,
}

#[derive(Debug, Clone)]
struct ResolvedRepositoryLocation {
    repository: GitRepository,
    location: ResolvedLocation,
    pathspec: Option<String>,
}

pub(crate) struct GitRepositoryResolver {
    authority: WorkspaceResolver,
}

impl GitRepositoryResolver {
    #[cfg(test)]
    fn new(workspace: &Path) -> Result<Self, ResolveError> {
        let authority = crate::workspace::WorkspaceResolver::active_workspace(workspace)
            .map_err(authority_error)?;
        Ok(Self::from_authority(authority))
    }

    fn from_authority(authority: WorkspaceResolver) -> Self {
        Self { authority }
    }

    fn resolve_existing(&self, raw: &str) -> Result<ResolvedLocation, ResolveError> {
        self.resolve(raw, false)
    }

    fn resolve_allow_missing(&self, raw: &str) -> Result<ResolvedLocation, ResolveError> {
        self.resolve(raw, true)
    }

    fn resolve(&self, raw: &str, allow_missing: bool) -> Result<ResolvedLocation, ResolveError> {
        let candidate = self.authority.input_path(raw).map_err(authority_error)?;

        let (existing, suffix, full_exists) = nearest_existing_ancestor(&candidate)?;
        if !allow_missing && !full_exists {
            return Err(ResolveError::NotFound);
        }
        let canonical_existing = fs::canonicalize(&existing).map_err(|_| ResolveError::NotFound)?;
        if !self.authority.allows_canonical(&canonical_existing) {
            return Err(ResolveError::OutsideWorkspace);
        }
        let metadata = fs::metadata(&existing).map_err(|_| ResolveError::NotFound)?;
        if !metadata.is_dir() && !suffix.is_empty() {
            return Err(ResolveError::NotFound);
        }
        let canonical = suffix
            .iter()
            .fold(canonical_existing.clone(), |path, part| path.join(part));
        if !self.authority.allows_canonical(&canonical) {
            return Err(ResolveError::OutsideWorkspace);
        }
        let full_metadata = if full_exists {
            fs::metadata(&candidate).ok()
        } else {
            None
        };
        let is_dir = full_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.is_dir());
        let discovery_dir = if full_exists && !is_dir {
            canonical
                .parent()
                .ok_or(ResolveError::InvalidPath)?
                .to_path_buf()
        } else if full_exists {
            canonical.clone()
        } else if metadata.is_dir() {
            canonical_existing
        } else {
            return Err(ResolveError::NotFound);
        };
        let display = self
            .authority
            .display_path(&canonical)
            .map_err(authority_error)?;
        Ok(ResolvedLocation {
            canonical,
            discovery_dir,
            is_dir,
            display: if display.is_empty() {
                ".".to_string()
            } else {
                display
            },
        })
    }

    fn repository_for(
        &self,
        location: ResolvedLocation,
    ) -> Result<Option<ResolvedRepositoryLocation>, ResolveError> {
        let Some(repository) = self.discover_repository(&location.discovery_dir)? else {
            return Ok(None);
        };
        let pathspec = location
            .canonical
            .strip_prefix(&repository.canonical_root)
            .map(path_to_slashes)
            .map_err(|_| ResolveError::RepositoryMismatch)?;
        Ok(Some(ResolvedRepositoryLocation {
            repository,
            location,
            pathspec: (!pathspec.is_empty()).then_some(pathspec),
        }))
    }

    fn discover_repository(&self, start: &Path) -> Result<Option<GitRepository>, ResolveError> {
        let mut current = start.to_path_buf();
        loop {
            if !self.authority.allows_canonical(&current) {
                return Err(ResolveError::OutsideWorkspace);
            }
            let marker = current.join(".git");
            if fs::symlink_metadata(&marker).is_ok() {
                validate_git_marker(&marker, &current, &self.authority)?;
                let validated = WorkspaceValidator
                    .validate(&current)
                    .map_err(|_| ResolveError::InvalidRepository)?;
                let execution_root = validated.execution_path().to_path_buf();
                if is_verbatim_path(&execution_root) {
                    return Err(ResolveError::InvalidRepository);
                }
                let canonical_execution = fs::canonicalize(&execution_root)
                    .map_err(|_| ResolveError::InvalidRepository)?;
                if canonical_execution != current
                    || !self.authority.allows_canonical(&canonical_execution)
                {
                    return Err(ResolveError::ReparseEscape);
                }
                return Ok(Some(GitRepository {
                    canonical_root: current,
                    execution_root,
                }));
            }
            if self.authority.discovery_stops_at(&current) {
                return Ok(None);
            }
            if !current.pop() {
                return Ok(None);
            }
        }
    }
}

fn nearest_existing_ancestor(
    candidate: &Path,
) -> Result<(PathBuf, Vec<OsString>, bool), ResolveError> {
    if fs::symlink_metadata(candidate).is_ok() {
        return Ok((candidate.to_path_buf(), Vec::new(), true));
    }
    let mut current = candidate.to_path_buf();
    let mut suffix = Vec::new();
    loop {
        let Some(name) = current.file_name().map(OsStr::to_os_string) else {
            return Err(ResolveError::NotFound);
        };
        suffix.insert(0, name);
        if !current.pop() {
            return Err(ResolveError::NotFound);
        }
        if fs::symlink_metadata(&current).is_ok() {
            return Ok((current, suffix, false));
        }
    }
}

fn validate_git_marker(
    marker: &Path,
    repository: &Path,
    authority: &WorkspaceResolver,
) -> Result<(), ResolveError> {
    let metadata = fs::symlink_metadata(marker).map_err(|_| ResolveError::InvalidRepository)?;
    if metadata.file_type().is_symlink() {
        return Err(ResolveError::ReparseEscape);
    }
    if metadata.is_dir() {
        let canonical = fs::canonicalize(marker).map_err(|_| ResolveError::InvalidRepository)?;
        return authority
            .allows_canonical(&canonical)
            .then_some(())
            .ok_or(ResolveError::ReparseEscape);
    }
    if !metadata.is_file() {
        return Err(ResolveError::InvalidRepository);
    }
    let content = fs::read_to_string(marker).map_err(|_| ResolveError::InvalidRepository)?;
    let target = content
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:").map(str::trim))
        .filter(|value| !value.is_empty())
        .ok_or(ResolveError::InvalidRepository)?;
    let target = Path::new(target);
    let target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        repository.join(target)
    };
    let canonical = fs::canonicalize(target).map_err(|_| ResolveError::InvalidRepository)?;
    authority
        .allows_canonical(&canonical)
        .then_some(())
        .ok_or(ResolveError::ReparseEscape)
}

fn git_status(resolver: &GitRepositoryResolver, arguments: &Map<String, Value>) -> Option<Value> {
    let raw = string_arg(arguments, "path").unwrap_or(".");
    let location = match resolver.resolve_existing(raw) {
        Ok(location) => location,
        Err(error) => return Some(resolve_error(error)),
    };
    let resolved = match resolver.repository_for(location) {
        Ok(Some(resolved)) => resolved,
        Ok(None) => return None,
        Err(error) => return Some(resolve_error(error)),
    };
    let max_entries = usize_arg(arguments, "max_entries", 1_000).clamp(1, 10_000);
    let include_untracked = bool_arg(arguments, "include_untracked", true);
    let mut args = vec![
        os("--no-pager"),
        os("status"),
        os("--porcelain=v1"),
        os("-b"),
        os("-z"),
    ];
    if !include_untracked {
        args.push(os("--untracked-files=no"));
    }
    let status = match run_git(&resolved.repository, &args, MAX_CAPTURE_BYTES) {
        Ok(output) if output.exit_code == 0 && !output.timed_out => output,
        Ok(output) => return Some(git_failure(&output)),
        Err(message) => return Some(tool_error("GIT_ERROR", &message)),
    };
    let ((branch, upstream, ahead, behind), mut entries) =
        match parse_status_porcelain_v1_z(&status.output, status.truncated) {
            Ok(parsed) => parsed,
            Err(message) => return Some(tool_error("GIT_ERROR", message)),
        };
    let truncated = status.truncated || entries.len() > max_entries;
    entries.truncate(max_entries);
    let head = run_git(
        &resolved.repository,
        &[os("--no-pager"), os("rev-parse"), os("HEAD")],
        256,
    )
    .ok()
    .filter(|output| output.exit_code == 0)
    .map(|output| String::from_utf8_lossy(&output.output).trim().to_string())
    .filter(|value| !value.is_empty());
    let clean = entries.is_empty();
    let repository_root = resolver
        .authority
        .display_path(&resolved.repository.canonical_root)
        .unwrap_or_else(|_| ".".to_string());
    let payload = json!({
        "is_repo": true,
        "path": resolved.location.display,
        "repository_root": if repository_root.is_empty() { "." } else { repository_root.as_str() },
        "branch": branch,
        "head": head,
        "upstream": upstream,
        "ahead": ahead,
        "behind": behind,
        "clean": clean,
        "entries": entries,
        "truncated": truncated
    });
    Some(tool_success("git_status", payload))
}

type BranchStatus = (Option<String>, Option<String>, u64, u64);

fn parse_status_porcelain_v1_z(
    bytes: &[u8],
    truncated: bool,
) -> Result<(BranchStatus, Vec<Value>), &'static str> {
    if bytes.is_empty() {
        return Ok(((None, None, 0, 0), Vec::new()));
    }
    if !truncated && bytes.last() != Some(&0) {
        return Err("Git status returned an incomplete machine record");
    }
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    let branch_record = records
        .next()
        .ok_or("Git status omitted its branch record")?;
    let branch_line =
        std::str::from_utf8(branch_record).map_err(|_| "Git status branch is not valid UTF-8")?;
    let branch = parse_branch_line(branch_line);
    let mut entries = Vec::new();
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if record.len() < 3 || record[2] != b' ' {
            if truncated && records.peek().is_none() {
                break;
            }
            return Err("Git status returned an invalid machine record");
        }
        let index_status = record[0] as char;
        let worktree_status = record[1] as char;
        let path = std::str::from_utf8(&record[3..])
            .map_err(|_| "Git status path is not valid UTF-8")?
            .to_string();
        let rename_or_copy =
            matches!(index_status, 'R' | 'C') || matches!(worktree_status, 'R' | 'C');
        let original_path = if rename_or_copy {
            let original = records
                .next()
                .filter(|record| !record.is_empty())
                .ok_or("Git status omitted the original rename path")?;
            Some(
                std::str::from_utf8(original)
                    .map_err(|_| "Git status original path is not valid UTF-8")?
                    .to_string(),
            )
        } else {
            None
        };
        entries.push(json!({
            "path": path,
            "original_path": original_path,
            "index_status": index_status.to_string(),
            "worktree_status": worktree_status.to_string(),
        }));
    }
    Ok((branch, entries))
}

fn git_diff(resolver: &GitRepositoryResolver, arguments: &Map<String, Value>) -> Option<Value> {
    let context_raw = string_arg(arguments, "path").unwrap_or(".");
    let context_location = match resolver.resolve_existing(context_raw) {
        Ok(location) => location,
        Err(error) => return Some(resolve_error(error)),
    };
    let context = match resolver.repository_for(context_location) {
        Ok(Some(resolved)) => resolved,
        Ok(None) => return None,
        Err(error) => return Some(resolve_error(error)),
    };
    let filters = path_filters(arguments);
    let pathspecs = match resolve_pathspecs_in_repository(resolver, &context.repository, &filters) {
        Ok(pathspecs) => pathspecs,
        Err(error) => return Some(resolve_error(error)),
    };
    let staged = bool_arg(arguments, "staged", false);
    let unstaged = bool_arg(arguments, "unstaged", true);
    let context_lines = usize_arg(arguments, "context_lines", 3).min(20);
    let max_bytes =
        usize_arg(arguments, "max_bytes", DEFAULT_TEXT_BYTES).clamp(1, MAX_CAPTURE_BYTES);
    let mut combined = String::new();
    let mut files = Vec::new();
    let mut command_truncated = false;
    for cached in [false, true] {
        if (!cached && !unstaged) || (cached && !staged) {
            continue;
        }
        let repository = &context.repository;
        let mut args = vec![
            os("--no-pager"),
            os("diff"),
            os("--no-ext-diff"),
            os("--no-textconv"),
            os(format!("--unified={context_lines}")),
        ];
        if cached {
            args.push(os("--cached"));
        }
        if !pathspecs.is_empty() {
            args.push(os("--"));
            args.extend(pathspecs.iter().map(os));
        }
        let output = match run_git(repository, &args, max_bytes) {
            Ok(output) if output.exit_code == 0 && !output.timed_out => output,
            Ok(output) => return Some(git_failure(&output)),
            Err(message) => return Some(tool_error("GIT_ERROR", &message)),
        };
        command_truncated |= output.truncated;
        if !combined.is_empty() && !output.output.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&String::from_utf8_lossy(&output.output));

        let mut name_args = vec![
            os("--no-pager"),
            os("diff"),
            os("--no-ext-diff"),
            os("--no-textconv"),
            os("--name-status"),
            os("-z"),
        ];
        let mut numstat_args = vec![
            os("--no-pager"),
            os("diff"),
            os("--no-ext-diff"),
            os("--no-textconv"),
            os("--numstat"),
            os("-z"),
        ];
        if cached {
            name_args.push(os("--cached"));
            numstat_args.push(os("--cached"));
        }
        if !pathspecs.is_empty() {
            name_args.push(os("--"));
            numstat_args.push(os("--"));
            name_args.extend(pathspecs.iter().map(os));
            numstat_args.extend(pathspecs.iter().map(os));
        }
        match machine_file_metadata(repository, &name_args, &numstat_args) {
            Ok(mut batch) => files.append(&mut batch),
            Err(error) => return Some(error),
        }
    }
    let (diff, text_truncated, output_bytes, output_lines) =
        truncate_text(&combined, max_bytes, DEFAULT_MAX_LINES);
    let truncated = command_truncated || text_truncated;
    let warnings = if truncated {
        vec!["diff truncated"]
    } else {
        Vec::<&str>::new()
    };
    Some(tool_success(
        "git_diff",
        json!({
            "diff": diff,
            "files": files,
            "truncated": truncated,
            "truncated_by": if truncated { Value::String("bytes_or_lines".into()) } else { Value::Null },
            "output_bytes": output_bytes,
            "output_lines": output_lines,
            "warnings": warnings
        }),
    ))
}

fn git_log(resolver: &GitRepositoryResolver, arguments: &Map<String, Value>) -> Option<Value> {
    let raw = string_arg(arguments, "path").unwrap_or(".");
    let location = match resolver.resolve_existing(raw) {
        Ok(location) => location,
        Err(error) => return Some(resolve_error(error)),
    };
    let resolved = match resolver.repository_for(location) {
        Ok(Some(resolved)) => resolved,
        Ok(None) => return None,
        Err(error) => return Some(resolve_error(error)),
    };
    let reference = string_arg(arguments, "ref").unwrap_or("HEAD");
    if !valid_git_ref(reference) {
        return Some(tool_error("INVALID_ARGUMENT", "无效 Git ref"));
    }
    let max_count = usize_arg(arguments, "max_count", 20).clamp(1, 100);
    let skip = usize_arg(arguments, "skip", 0);
    let args = vec![
        os("--no-pager"),
        os("log"),
        os(format!("--max-count={}", max_count + 1)),
        os(format!("--skip={skip}")),
        os("--date=iso-strict"),
        os("--pretty=format:%H%x1f%h%x1f%an%x1f%ae%x1f%ad%x1f%s%x1e"),
        os(reference),
    ];
    let output = match run_git(&resolved.repository, &args, MAX_CAPTURE_BYTES) {
        Ok(output) if output.exit_code == 0 && !output.timed_out => output,
        Ok(output) => return Some(git_failure(&output)),
        Err(message) => return Some(tool_error("GIT_ERROR", &message)),
    };
    let text = String::from_utf8_lossy(&output.output);
    let mut commits = text
        .split('\x1e')
        .filter_map(|record| {
            let fields = record
                .trim_matches(['\r', '\n'])
                .split('\x1f')
                .collect::<Vec<_>>();
            (fields.len() >= 6).then(|| {
                json!({
                    "hash": fields[0], "short_hash": fields[1], "author_name": fields[2],
                    "author_email": fields[3], "author_date": fields[4], "subject": fields[5]
                })
            })
        })
        .collect::<Vec<_>>();
    let truncated = output.truncated || commits.len() > max_count;
    commits.truncate(max_count);
    let next_action = truncated.then(|| json!({
        "tool": "git_log",
        "arguments": {"path": raw, "ref": reference, "max_count": max_count, "skip": skip + max_count}
    }));
    Some(tool_success(
        "git_log",
        json!({
            "is_repo": true,
            "ref": reference,
            "path": resolved.location.display,
            "max_count": max_count,
            "skip": skip,
            "commits": commits,
            "truncated": truncated,
            "warnings": if output.truncated { vec!["git log output truncated"] } else { Vec::<&str>::new() },
            "next_action": next_action
        }),
    ))
}

fn git_show(resolver: &GitRepositoryResolver, arguments: &Map<String, Value>) -> Option<Value> {
    let context_raw = string_arg(arguments, "path").unwrap_or(".");
    let context_location = match resolver.resolve_existing(context_raw) {
        Ok(location) => location,
        Err(error) => return Some(resolve_error(error)),
    };
    let context = match resolver.repository_for(context_location) {
        Ok(Some(resolved)) => resolved,
        Ok(None) => return None,
        Err(error) => return Some(resolve_error(error)),
    };
    let filters = path_filters(arguments);
    let pathspecs = match resolve_pathspecs_in_repository(resolver, &context.repository, &filters) {
        Ok(pathspecs) => pathspecs,
        Err(error) => return Some(resolve_error(error)),
    };
    let reference = string_arg(arguments, "rev").unwrap_or("HEAD");
    if !valid_git_ref(reference) {
        return Some(tool_error("INVALID_ARGUMENT", "无效 Git rev"));
    }
    let context_lines = usize_arg(arguments, "context_lines", 3).min(20);
    let max_bytes =
        usize_arg(arguments, "max_bytes", DEFAULT_TEXT_BYTES).clamp(1, MAX_CAPTURE_BYTES);
    let include_patch = bool_arg(arguments, "include_patch", true);
    let mut args = vec![
        os("--no-pager"),
        os("show"),
        os("--no-ext-diff"),
        os("--no-textconv"),
        os("--format=fuller"),
        os(format!("--unified={context_lines}")),
    ];
    if !include_patch {
        args.push(os("--no-patch"));
    }
    args.push(os(reference));
    if !pathspecs.is_empty() {
        args.push(os("--"));
        args.extend(pathspecs.iter().map(os));
    }
    let output = match run_git(&context.repository, &args, max_bytes) {
        Ok(output) if output.exit_code == 0 && !output.timed_out => output,
        Ok(output) => return Some(git_failure(&output)),
        Err(message) => return Some(tool_error("GIT_ERROR", &message)),
    };
    let raw = String::from_utf8_lossy(&output.output);
    let mut name_args = vec![
        os("--no-pager"),
        os("show"),
        os("--no-ext-diff"),
        os("--no-textconv"),
        os("--format="),
        os("--name-status"),
        os("-z"),
        os(reference),
    ];
    let mut numstat_args = vec![
        os("--no-pager"),
        os("show"),
        os("--no-ext-diff"),
        os("--no-textconv"),
        os("--format="),
        os("--numstat"),
        os("-z"),
        os(reference),
    ];
    if !pathspecs.is_empty() {
        name_args.push(os("--"));
        numstat_args.push(os("--"));
        name_args.extend(pathspecs.iter().map(os));
        numstat_args.extend(pathspecs.iter().map(os));
    }
    let files = match machine_file_metadata(&context.repository, &name_args, &numstat_args) {
        Ok(files) => files,
        Err(error) => return Some(error),
    };
    let (content, text_truncated, output_bytes, output_lines) =
        truncate_text(&raw, max_bytes, DEFAULT_MAX_LINES);
    let truncated = output.truncated || text_truncated;
    Some(tool_success(
        "git_show",
        json!({
            "is_repo": true,
            "rev": reference,
            "content": content,
            "files": files,
            "truncated": truncated,
            "truncated_by": if truncated { Value::String("bytes_or_lines".into()) } else { Value::Null },
            "output_bytes": output_bytes,
            "output_lines": output_lines,
            "warnings": if truncated { vec!["git show output truncated"] } else { Vec::<&str>::new() }
        }),
    ))
}

fn git_blame(resolver: &GitRepositoryResolver, arguments: &Map<String, Value>) -> Option<Value> {
    let Some(raw) = string_arg(arguments, "path") else {
        return Some(tool_error("INVALID_ARGUMENT", "git_blame 需要 path"));
    };
    let location = match resolver.resolve_existing(raw) {
        Ok(location) => location,
        Err(error) => return Some(resolve_error(error)),
    };
    if location.is_dir {
        return Some(tool_error("IS_DIRECTORY", "git_blame path 必须是文件"));
    }
    let resolved = match resolver.repository_for(location) {
        Ok(Some(resolved)) => resolved,
        Ok(None) => return None,
        Err(error) => return Some(resolve_error(error)),
    };
    let Some(pathspec) = resolved.pathspec.as_deref() else {
        return Some(tool_error(
            "INVALID_ARGUMENT",
            "git_blame path 必须是仓库文件",
        ));
    };
    let reference = string_arg(arguments, "rev");
    if reference.is_some_and(|value| !valid_git_ref(value)) {
        return Some(tool_error("INVALID_ARGUMENT", "无效 Git rev"));
    }
    let start_line = usize_arg(arguments, "start_line", 1).max(1);
    let max_lines = usize_arg(arguments, "max_lines", 200).clamp(1, 2_000);
    let requested_end = arguments
        .get("end_line")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let max_end = start_line.saturating_add(max_lines).saturating_sub(1);
    let requested_end = requested_end.unwrap_or(max_end);
    if requested_end < start_line {
        return Some(tool_error(
            "INVALID_ARGUMENT",
            "end_line 不能小于 start_line",
        ));
    }
    let end_line = requested_end.min(max_end);
    let mut args = vec![
        os("--no-pager"),
        os("blame"),
        os("--line-porcelain"),
        os("--no-textconv"),
        os("-L"),
        os(format!("{start_line},{end_line}")),
    ];
    if let Some(reference) = reference {
        args.push(os(reference));
    }
    args.push(os("--"));
    args.push(os(pathspec));
    let output = match run_git(&resolved.repository, &args, MAX_CAPTURE_BYTES) {
        Ok(output) if output.exit_code == 0 && !output.timed_out => output,
        Ok(output) => return Some(git_failure(&output)),
        Err(message) => return Some(tool_error("GIT_ERROR", &message)),
    };
    let mut lines = parse_blame_porcelain(&String::from_utf8_lossy(&output.output));
    let truncated = output.truncated || requested_end > end_line || lines.len() > max_lines;
    lines.truncate(max_lines);
    let actual_end = lines
        .last()
        .and_then(|line| line.get("line"))
        .and_then(Value::as_u64);
    let next_action = truncated.then(|| json!({
        "tool":"git_blame",
        "arguments":{"path":raw,"rev":reference,"start_line":actual_end.unwrap_or(end_line as u64)+1,"max_lines":max_lines}
    }));
    Some(tool_success(
        "git_blame",
        json!({
            "is_repo": true,
            "path": resolved.location.display,
            "rev": reference,
            "start_line": start_line,
            "end_line": actual_end,
            "max_lines": max_lines,
            "lines": lines,
            "truncated": truncated,
            "warnings": if output.truncated { vec!["git blame output truncated"] } else { Vec::<&str>::new() },
            "next_action": next_action
        }),
    ))
}

fn resolve_error(error: ResolveError) -> Value {
    match error {
        ResolveError::NotFound => tool_error("NOT_FOUND", "Git 路径不存在"),
        ResolveError::OutsideWorkspace | ResolveError::ReparseEscape => {
            tool_error("OUTSIDE_WORKSPACE", "Git 路径超出已授权工作区")
        }
        ResolveError::RepositoryMismatch => {
            tool_error("INVALID_ARGUMENT", "Git 路径不属于同一仓库")
        }
        ResolveError::InvalidRepository => tool_error("GIT_ERROR", "Git 仓库元数据无效"),
        ResolveError::InvalidPath => tool_error("INVALID_ARGUMENT", "Git 路径无效"),
    }
}

fn authority_error(error: PathAuthorityError) -> ResolveError {
    match error {
        PathAuthorityError::InvalidPath => ResolveError::InvalidPath,
        PathAuthorityError::NotFound => ResolveError::NotFound,
        PathAuthorityError::OutsideAuthority => ResolveError::OutsideWorkspace,
    }
}

#[derive(Debug)]
struct GitOutput {
    exit_code: u32,
    output: Vec<u8>,
    truncated: bool,
    timed_out: bool,
}

fn run_git(
    repository: &GitRepository,
    args: &[OsString],
    max_bytes: usize,
) -> Result<GitOutput, String> {
    let executable = find_git_executable().ok_or_else(|| "找不到可执行的 git.exe".to_string())?;
    let output = run_bounded_command(
        &executable,
        args,
        &repository.execution_root,
        GIT_TIMEOUT,
        max_bytes.clamp(1, MAX_CAPTURE_BYTES),
    )
    .map_err(|error| format!("Git 进程执行失败: {error}"))?;
    Ok(GitOutput {
        exit_code: output.exit_code,
        output: output.output,
        truncated: output.truncated,
        timed_out: output.timed_out,
    })
}

fn find_git_executable() -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .filter(|directory| directory.is_absolute() && !is_verbatim_path(directory))
        .map(|directory| directory.join("git.exe"))
        .find(|candidate| !is_verbatim_path(candidate) && candidate.is_file())
}

fn git_failure(output: &GitOutput) -> Value {
    if output.timed_out {
        return tool_error("GIT_TIMEOUT", "Git 命令执行超时");
    }
    let message = String::from_utf8_lossy(&output.output);
    let message = message.trim();
    tool_error(
        "GIT_ERROR",
        if message.is_empty() {
            "Git 命令执行失败"
        } else {
            message
        },
    )
}

fn tool_success(name: &str, mut payload: Value) -> Value {
    if let Some(object) = payload.as_object_mut() {
        object.insert("ok".into(), Value::Bool(true));
    }
    let text = render_text(name, &payload);
    json!({
        "content": [{"type":"text","text":text}],
        "structuredContent": payload,
        "isError": false
    })
}

fn tool_error(code: &str, message: &str) -> Value {
    let payload = json!({
        "ok": false,
        "error": {"code":code,"message":message,"category":"runtime","retryable":false,"details":{}}
    });
    json!({
        "content": [{"type":"text","text":format!("{code}: {message}")}],
        "structuredContent": payload,
        "isError": true
    })
}

fn render_text(name: &str, payload: &Value) -> String {
    match name {
        "git_status" => {
            let branch = payload
                .get("branch")
                .and_then(Value::as_str)
                .unwrap_or("detached");
            let entries = payload
                .get("entries")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut lines = vec![format!("## {branch}")];
            for entry in entries {
                lines.push(format!(
                    "{}{} {}",
                    entry
                        .get("index_status")
                        .and_then(Value::as_str)
                        .unwrap_or(" "),
                    entry
                        .get("worktree_status")
                        .and_then(Value::as_str)
                        .unwrap_or(" "),
                    entry.get("path").and_then(Value::as_str).unwrap_or("")
                ));
            }
            if lines.len() == 1 {
                lines.push("Working tree clean.".into());
            }
            lines.join("\n")
        }
        "git_diff" => payload
            .get("diff")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("No diff.")
            .to_string(),
        "git_show" => payload
            .get("content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("No output.")
            .to_string(),
        "git_log" => payload
            .get("commits")
            .and_then(Value::as_array)
            .map(|commits| {
                commits
                    .iter()
                    .map(|commit| {
                        format!(
                            "{} {}",
                            commit
                                .get("short_hash")
                                .and_then(Value::as_str)
                                .unwrap_or(""),
                            commit.get("subject").and_then(Value::as_str).unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "No commits found.".into()),
        "git_blame" => payload
            .get("lines")
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .map(|line| {
                        format!(
                            "{} {} {}",
                            line.get("line").and_then(Value::as_u64).unwrap_or(0),
                            line.get("commit").and_then(Value::as_str).unwrap_or(""),
                            line.get("content").and_then(Value::as_str).unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "No blame lines found.".into()),
        _ => String::new(),
    }
}

fn parse_branch_line(line: &str) -> (Option<String>, Option<String>, u64, u64) {
    let raw = line.strip_prefix("## ").unwrap_or(line).trim();
    let raw = raw.strip_prefix("No commits yet on ").unwrap_or(raw);
    let (main, bracket) = raw
        .split_once(" [")
        .map(|(main, tail)| (main, Some(tail.trim_end_matches(']'))))
        .unwrap_or((raw, None));
    let (branch, upstream) = main
        .split_once("...")
        .map(|(branch, upstream)| (branch, Some(upstream)))
        .unwrap_or((main, None));
    let mut ahead = 0;
    let mut behind = 0;
    if let Some(bracket) = bracket {
        for item in bracket.split(',').map(str::trim) {
            if let Some(value) = item.strip_prefix("ahead ") {
                ahead = value.parse().unwrap_or(0);
            }
            if let Some(value) = item.strip_prefix("behind ") {
                behind = value.parse().unwrap_or(0);
            }
        }
    }
    (
        Some(branch.to_string()).filter(|value| !value.is_empty()),
        upstream.map(str::to_string),
        ahead,
        behind,
    )
}

fn machine_file_metadata(
    repository: &GitRepository,
    name_status_args: &[OsString],
    numstat_args: &[OsString],
) -> Result<Vec<Value>, Value> {
    let names = run_git(repository, name_status_args, MAX_CAPTURE_BYTES)
        .map_err(|message| tool_error("GIT_ERROR", &message))?;
    if names.exit_code != 0 || names.timed_out {
        return Err(git_failure(&names));
    }
    if names.truncated {
        return Err(tool_error(
            "GIT_ERROR",
            "Git file metadata exceeded the bounded capture limit",
        ));
    }
    let numstat = run_git(repository, numstat_args, MAX_CAPTURE_BYTES)
        .map_err(|message| tool_error("GIT_ERROR", &message))?;
    if numstat.exit_code != 0 || numstat.timed_out {
        return Err(git_failure(&numstat));
    }
    if numstat.truncated {
        return Err(tool_error(
            "GIT_ERROR",
            "Git binary metadata exceeded the bounded capture limit",
        ));
    }
    let binaries = parse_numstat_binary_paths_z(&numstat.output);
    let mut files = parse_name_status_z(&names.output);
    for file in &mut files {
        let path = file.get("path").and_then(Value::as_str).unwrap_or_default();
        file["binary"] = Value::Bool(binaries.contains(path));
    }
    Ok(files)
}

fn parse_name_status_z(bytes: &[u8]) -> Vec<Value> {
    let fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let mut files = Vec::new();
    let mut index = 0usize;
    while index < fields.len() {
        let status = String::from_utf8_lossy(fields[index]);
        index += 1;
        let kind = status.chars().next().unwrap_or('M');
        let path = if matches!(kind, 'R' | 'C') {
            if index + 1 >= fields.len() {
                break;
            }
            index += 1;
            let destination = String::from_utf8_lossy(fields[index]).into_owned();
            index += 1;
            destination
        } else {
            let Some(field) = fields.get(index) else {
                break;
            };
            index += 1;
            String::from_utf8_lossy(field).into_owned()
        };
        let status = match kind {
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            'C' => "copied",
            'T' => "type_changed",
            'U' => "unmerged",
            _ => "modified",
        };
        files.push(json!({"path":path,"status":status,"binary":false}));
    }
    files
}

fn parse_numstat_binary_paths_z(bytes: &[u8]) -> HashSet<String> {
    let mut binaries = HashSet::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let Some(end) = bytes[cursor..].iter().position(|byte| *byte == 0) else {
            break;
        };
        let record_end = cursor + end;
        let record = &bytes[cursor..record_end];
        cursor = record_end + 1;
        if record.is_empty() {
            continue;
        }
        let mut parts = record.splitn(3, |byte| *byte == b'\t');
        let added = parts.next().unwrap_or_default();
        let deleted = parts.next().unwrap_or_default();
        let inline_path = parts.next().unwrap_or_default();
        let path = if inline_path.is_empty() {
            let Some(old_end) = bytes[cursor..].iter().position(|byte| *byte == 0) else {
                break;
            };
            cursor += old_end + 1;
            let Some(new_end) = bytes[cursor..].iter().position(|byte| *byte == 0) else {
                break;
            };
            let new_path = &bytes[cursor..cursor + new_end];
            cursor += new_end + 1;
            new_path
        } else {
            inline_path
        };
        if added == b"-" && deleted == b"-" {
            binaries.insert(String::from_utf8_lossy(path).into_owned());
        }
    }
    binaries
}

fn parse_blame_porcelain(text: &str) -> Vec<Value> {
    let mut rows = Vec::new();
    let mut current: HashMap<&'static str, Value> = HashMap::new();
    for line in text.lines() {
        if let Some(content) = line.strip_prefix('\t') {
            if !current.is_empty() {
                current.insert("content", Value::String(content.to_string()));
                rows.push(json!({
                    "commit": current.get("commit").cloned().unwrap_or(Value::Null),
                    "original_line": current.get("original_line").cloned().unwrap_or(Value::Null),
                    "line": current.get("line").cloned().unwrap_or(Value::Null),
                    "author": current.get("author").cloned().unwrap_or(Value::Null),
                    "author_mail": current.get("author_mail").cloned().unwrap_or(Value::Null),
                    "author_time": current.get("author_time").cloned().unwrap_or(Value::Null),
                    "summary": current.get("summary").cloned().unwrap_or(Value::Null),
                    "content": current.get("content").cloned().unwrap_or(Value::Null)
                }));
                current.clear();
            }
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let hash = fields
            .first()
            .copied()
            .unwrap_or_default()
            .trim_start_matches('^');
        if (hash.len() == 40 && hash.chars().all(|ch| ch.is_ascii_hexdigit())) && fields.len() >= 3
        {
            current.clear();
            current.insert("commit", Value::String(hash.to_string()));
            current.insert(
                "original_line",
                Value::from(fields[1].parse::<u64>().unwrap_or(0)),
            );
            current.insert("line", Value::from(fields[2].parse::<u64>().unwrap_or(0)));
            continue;
        }
        for (prefix, key) in [
            ("author ", "author"),
            ("author-mail ", "author_mail"),
            ("author-time ", "author_time"),
            ("summary ", "summary"),
        ] {
            if let Some(value) = line.strip_prefix(prefix) {
                current.insert(key, Value::String(value.to_string()));
                break;
            }
        }
    }
    rows
}

fn truncate_text(text: &str, max_bytes: usize, max_lines: usize) -> (String, bool, usize, usize) {
    let original_bytes = text.len();
    let original_lines = text.lines().count();
    let mut kept = String::new();
    let mut truncated = false;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if index >= max_lines {
            truncated = true;
            break;
        }
        let remaining = max_bytes.saturating_sub(kept.len());
        if line.len() <= remaining {
            kept.push_str(line);
        } else {
            let bytes = line.as_bytes();
            let mut end = remaining.min(bytes.len());
            while end > 0 && !line.is_char_boundary(end) {
                end -= 1;
            }
            kept.push_str(&line[..end]);
            truncated = true;
            break;
        }
    }
    if kept.len() < original_bytes {
        truncated = true;
    }
    (kept, truncated, original_bytes, original_lines)
}

fn path_filters(arguments: &Map<String, Value>) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(paths) = arguments.get("paths").and_then(Value::as_array) {
        values.extend(paths.iter().filter_map(Value::as_str).map(str::to_string));
    }
    values
}

fn resolve_pathspecs_in_repository(
    resolver: &GitRepositoryResolver,
    repository: &GitRepository,
    filters: &[String],
) -> Result<Vec<String>, ResolveError> {
    let mut pathspecs = Vec::new();
    for raw in filters {
        let location = resolver.resolve_allow_missing(raw)?;
        let Some(resolved) = resolver.repository_for(location)? else {
            return Err(ResolveError::RepositoryMismatch);
        };
        if resolved.repository.canonical_root != repository.canonical_root {
            return Err(ResolveError::RepositoryMismatch);
        }
        if let Some(pathspec) = resolved.pathspec {
            pathspecs.push(pathspec);
        }
    }
    Ok(pathspecs)
}

fn valid_git_ref(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-') && !value.contains(['\0', '\n', '\r'])
}

fn string_arg<'a>(arguments: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(Value::as_str)
}
fn bool_arg(arguments: &Map<String, Value>, key: &str, default: bool) -> bool {
    arguments
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}
fn usize_arg(arguments: &Map<String, Value>, key: &str, default: usize) -> usize {
    arguments
        .get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(default)
}
fn os(value: impl AsRef<OsStr>) -> OsString {
    value.as_ref().to_os_string()
}
fn path_to_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(windows)]
fn is_verbatim_path(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    let prefix = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    path.as_os_str().encode_wide().take(prefix.len()).eq(prefix)
}

#[cfg(not(windows))]
fn is_verbatim_path(_path: &Path) -> bool {
    false
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "localbridge-schema28-blame-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn git(root: &Path, args: &[&str]) {
        let executable = find_git_executable().expect("git.exe for schema28 blame fixture");
        let args = args.iter().map(os).collect::<Vec<_>>();
        let output =
            run_bounded_command(&executable, &args, root, Duration::from_secs(5), 64 * 1024)
                .unwrap();
        assert_eq!(
            output.exit_code,
            0,
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.output)
        );
    }

    #[test]
    fn schema33_git_resolver_keeps_workspace_and_broker_path_domains_separate() {
        let base = temp_repo();
        let workspace = base.join("workspace");
        let outside_repo = base.join("outside-repo");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&outside_repo).unwrap();
        git(&outside_repo, &["init"]);

        let workspace_resolver = GitRepositoryResolver::new(&workspace).unwrap();
        assert!(matches!(
            workspace_resolver.resolve_existing(outside_repo.to_string_lossy().as_ref()),
            Err(ResolveError::OutsideWorkspace)
        ));

        let broker_resolver =
            GitRepositoryResolver::from_authority(WorkspaceResolver::broker_administrator());
        let location = broker_resolver
            .resolve_existing(outside_repo.to_string_lossy().as_ref())
            .unwrap();
        let resolved = broker_resolver
            .repository_for(location)
            .unwrap()
            .expect("outside repository");
        assert_eq!(
            resolved.repository.canonical_root,
            fs::canonicalize(&outside_repo).unwrap()
        );
        assert!(
            broker_resolver
                .authority
                .display_path(&resolved.repository.canonical_root)
                .unwrap()
                .contains("outside-repo")
        );

        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn blame_ranges_are_one_based_inclusive_and_invalid_reverse_range_fails_closed() {
        let root = temp_repo();
        git(&root, &["init"]);
        git(&root, &["config", "user.email", "schema28@example.invalid"]);
        git(&root, &["config", "user.name", "LocalBridge Schema28"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        fs::write(
            root.join("blame.txt"),
            b"line1\nline2\nline3\nline4\nline5\nline6\n",
        )
        .unwrap();
        git(&root, &["add", "blame.txt"]);
        git(&root, &["commit", "-m", "schema28 blame fixture"]);

        let one = handle_git_tool(
            &root,
            "git_blame",
            &json!({"path":"blame.txt","start_line":5,"end_line":5,"max_lines":200}),
        )
        .unwrap();
        assert_eq!(one["isError"], false, "{one:#?}");
        let rows = one["structuredContent"]["lines"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{one:#?}");
        assert_eq!(rows[0]["line"], 5);
        assert_eq!(rows[0]["content"], "line5");
        assert_eq!(one["structuredContent"]["start_line"], 5);
        assert_eq!(one["structuredContent"]["end_line"], 5);

        let three = handle_git_tool(
            &root,
            "git_blame",
            &json!({"path":"blame.txt","start_line":1,"end_line":3,"max_lines":200}),
        )
        .unwrap();
        let rows = three["structuredContent"]["lines"].as_array().unwrap();
        assert_eq!(rows.len(), 3, "{three:#?}");
        assert_eq!(
            rows.iter()
                .map(|row| row["line"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        let invalid = handle_git_tool(
            &root,
            "git_blame",
            &json!({"path":"blame.txt","start_line":5,"end_line":3,"max_lines":200}),
        )
        .unwrap();
        assert_eq!(invalid["isError"], true);
        assert_eq!(
            invalid["structuredContent"]["error"]["code"],
            "INVALID_ARGUMENT"
        );

        let past_eof = handle_git_tool(
            &root,
            "git_blame",
            &json!({"path":"blame.txt","start_line":999,"end_line":1000,"max_lines":200}),
        )
        .unwrap();
        assert_eq!(past_eof["isError"], true, "{past_eof:#?}");

        for (tool, arguments) in [
            ("git_show", json!({"path":".","rev":"definitely-not-a-ref"})),
            ("git_log", json!({"path":".","ref":"definitely-not-a-ref"})),
        ] {
            let missing = handle_git_tool(&root, tool, &arguments).unwrap();
            assert_eq!(missing["isError"], true, "{tool}: {missing:#?}");
            assert_eq!(missing["structuredContent"]["ok"], false);
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_show_unicode_deleted_path_uses_nul_delimited_machine_metadata() {
        let root = temp_repo();
        git(&root, &["init"]);
        git(&root, &["config", "user.email", "unicode@example.invalid"]);
        git(&root, &["config", "user.name", "LocalBridge Unicode"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        fs::write(root.join("B.txt"), b"before\n").unwrap();
        fs::write(root.join("中文.txt"), b"delete me\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-m", "unicode base"]);
        fs::write(root.join("B.txt"), b"after\n").unwrap();
        fs::remove_file(root.join("中文.txt")).unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-m", "unicode delete"]);

        let shown = handle_git_tool(
            &root,
            "git_show",
            &json!({"path":".","rev":"HEAD","include_patch":true}),
        )
        .unwrap();
        assert_eq!(shown["isError"], false, "{shown:#?}");
        let files = shown["structuredContent"]["files"].as_array().unwrap();
        assert!(
            files
                .iter()
                .any(|file| file["path"] == "B.txt" && file["status"] == "modified"),
            "{files:#?}"
        );
        assert!(
            files
                .iter()
                .any(|file| file["path"] == "中文.txt" && file["status"] == "deleted"),
            "{files:#?}"
        );
        assert!(
            !files
                .iter()
                .any(|file| file["path"] == "B.txt" && file["status"] == "deleted"),
            "{files:#?}"
        );
        assert!(
            shown["structuredContent"]["content"]
                .as_str()
                .is_some_and(|content| content.contains("deleted file mode"))
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_status_preserves_unicode_arrow_and_rename_paths() {
        let root = temp_repo();
        git(&root, &["init"]);
        git(&root, &["config", "user.email", "status@example.invalid"]);
        git(&root, &["config", "user.name", "LocalBridge Status"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        let original = "测试 old.txt";
        let renamed = "重命名 new.txt";
        let untracked = "未跟踪 文件.txt";
        fs::write(root.join(original), b"tracked\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-m", "status fixture"]);
        git(&root, &["mv", original, renamed]);
        fs::write(root.join(untracked), b"untracked\n").unwrap();

        let status = handle_git_tool(
            &root,
            "git_status",
            &json!({"path":".","include_untracked":true}),
        )
        .unwrap();
        assert_eq!(status["isError"], false, "{status:#?}");
        let entries = status["structuredContent"]["entries"].as_array().unwrap();
        assert!(
            entries
                .iter()
                .any(|entry| { entry["path"] == renamed && entry["original_path"] == original }),
            "{entries:#?}"
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry["path"] == untracked && entry["original_path"].is_null()),
            "{entries:#?}"
        );
        assert!(entries.iter().all(|entry| {
            !entry["path"]
                .as_str()
                .is_some_and(|path| path.contains("\\346") || path.starts_with('"'))
        }));
        let machine = b"## main\0R  to -> literal.txt\0from -> literal.txt\0";
        let (_, special) = parse_status_porcelain_v1_z(machine, false).unwrap();
        assert_eq!(special[0]["path"], "to -> literal.txt");
        assert_eq!(special[0]["original_path"], "from -> literal.txt");
        fs::remove_dir_all(root).unwrap();
    }
}
