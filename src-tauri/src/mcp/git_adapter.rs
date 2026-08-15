use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::runtime::run_bounded_command;
use crate::workspace::WorkspaceValidator;

const GIT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
const DEFAULT_TEXT_BYTES: usize = 256 * 1024;
const DEFAULT_MAX_LINES: usize = 2_000;

pub(crate) fn handle_git_tool(workspace: &Path, name: &str, arguments: &Value) -> Option<Value> {
    if !matches!(
        name,
        "git_status" | "git_diff" | "git_log" | "git_show" | "git_blame"
    ) {
        return None;
    }
    let Some(arguments) = arguments.as_object() else {
        return Some(tool_error("INVALID_ARGUMENT", "Git 工具参数必须是对象"));
    };
    let resolver = match GitRepositoryResolver::new(workspace) {
        Ok(resolver) => resolver,
        Err(error) => return Some(resolve_error(error)),
    };
    match name {
        "git_status" => git_status(&resolver, arguments),
        "git_diff" => git_diff(&resolver, arguments),
        "git_log" => git_log(&resolver, arguments),
        "git_show" => git_show(&resolver, arguments),
        "git_blame" => git_blame(&resolver, arguments),
        _ => None,
    }
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
    workspace: PathBuf,
    canonical_workspace: PathBuf,
}

impl GitRepositoryResolver {
    fn new(workspace: &Path) -> Result<Self, ResolveError> {
        if !workspace.is_absolute() || is_verbatim_path(workspace) || !workspace.is_dir() {
            return Err(ResolveError::InvalidPath);
        }
        let canonical_workspace =
            fs::canonicalize(workspace).map_err(|_| ResolveError::InvalidPath)?;
        Ok(Self {
            workspace: workspace.to_path_buf(),
            canonical_workspace,
        })
    }

    fn resolve_existing(&self, raw: &str) -> Result<ResolvedLocation, ResolveError> {
        self.resolve(raw, false)
    }

    fn resolve_allow_missing(&self, raw: &str) -> Result<ResolvedLocation, ResolveError> {
        self.resolve(raw, true)
    }

    fn resolve(&self, raw: &str, allow_missing: bool) -> Result<ResolvedLocation, ResolveError> {
        if raw.is_empty() || raw.contains('\0') || raw.starts_with(r"\\?\") {
            return Err(ResolveError::InvalidPath);
        }
        let supplied = Path::new(raw);
        if supplied
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(ResolveError::InvalidPath);
        }
        let candidate = if supplied.is_absolute() {
            supplied.to_path_buf()
        } else {
            self.workspace.join(supplied)
        };
        if is_verbatim_path(&candidate) {
            return Err(ResolveError::InvalidPath);
        }

        let (existing, suffix, full_exists) = nearest_existing_ancestor(&candidate)?;
        if !allow_missing && !full_exists {
            return Err(ResolveError::NotFound);
        }
        let canonical_existing = fs::canonicalize(&existing).map_err(|_| ResolveError::NotFound)?;
        if !canonical_existing.starts_with(&self.canonical_workspace) {
            return Err(ResolveError::OutsideWorkspace);
        }
        let metadata = fs::metadata(&existing).map_err(|_| ResolveError::NotFound)?;
        if !metadata.is_dir() && !suffix.is_empty() {
            return Err(ResolveError::NotFound);
        }
        let canonical = suffix
            .iter()
            .fold(canonical_existing.clone(), |path, part| path.join(part));
        if !canonical.starts_with(&self.canonical_workspace) {
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
        let display = canonical
            .strip_prefix(&self.canonical_workspace)
            .map(path_to_slashes)
            .unwrap_or_else(|_| raw.replace('\\', "/"));
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
            if !current.starts_with(&self.canonical_workspace) {
                return Err(ResolveError::OutsideWorkspace);
            }
            let marker = current.join(".git");
            if fs::symlink_metadata(&marker).is_ok() {
                validate_git_marker(&marker, &current, &self.canonical_workspace)?;
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
                    || !canonical_execution.starts_with(&self.canonical_workspace)
                {
                    return Err(ResolveError::ReparseEscape);
                }
                return Ok(Some(GitRepository {
                    canonical_root: current,
                    execution_root,
                }));
            }
            if current == self.canonical_workspace {
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
    workspace: &Path,
) -> Result<(), ResolveError> {
    let metadata = fs::symlink_metadata(marker).map_err(|_| ResolveError::InvalidRepository)?;
    if metadata.file_type().is_symlink() {
        return Err(ResolveError::ReparseEscape);
    }
    if metadata.is_dir() {
        let canonical = fs::canonicalize(marker).map_err(|_| ResolveError::InvalidRepository)?;
        return canonical
            .starts_with(workspace)
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
    canonical
        .starts_with(workspace)
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
    ];
    if !include_untracked {
        args.push(os("--untracked-files=no"));
    }
    let status = match run_git(&resolved.repository, &args, MAX_CAPTURE_BYTES) {
        Ok(output) if output.exit_code == 0 && !output.timed_out => output,
        Ok(output) => return Some(git_failure(&output)),
        Err(message) => return Some(tool_error("GIT_ERROR", &message)),
    };
    let text = String::from_utf8_lossy(&status.output);
    let mut lines = text.lines();
    let branch_line = lines.next().unwrap_or_default();
    let (branch, upstream, ahead, behind) = parse_branch_line(branch_line);
    let mut entries = Vec::new();
    for line in lines {
        if line.len() < 3 {
            continue;
        }
        let bytes = line.as_bytes();
        let raw_path = line.get(3..).unwrap_or_default();
        let (original_path, path) = raw_path
            .split_once(" -> ")
            .map(|(from, to)| (Some(from.to_string()), to.to_string()))
            .unwrap_or((None, raw_path.to_string()));
        entries.push(json!({
            "path": path,
            "original_path": original_path,
            "index_status": (bytes[0] as char).to_string(),
            "worktree_status": (bytes[1] as char).to_string()
        }));
    }
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
    let repository_root = resolved
        .repository
        .canonical_root
        .strip_prefix(&resolver.canonical_workspace)
        .map(path_to_slashes)
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
    }
    let (diff, text_truncated, output_bytes, output_lines) =
        truncate_text(&combined, max_bytes, DEFAULT_MAX_LINES);
    let truncated = command_truncated || text_truncated;
    let files = parse_diff_files(&diff);
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
    let (content, text_truncated, output_bytes, output_lines) =
        truncate_text(&raw, max_bytes, DEFAULT_MAX_LINES);
    let truncated = output.truncated || text_truncated;
    Some(tool_success(
        "git_show",
        json!({
            "is_repo": true,
            "rev": reference,
            "content": content,
            "files": parse_diff_files(&raw),
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

fn parse_diff_files(diff: &str) -> Vec<Value> {
    let mut files: Vec<Value> = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            let path = rest.split(" b/").nth(1).unwrap_or(rest).to_string();
            files.push(json!({"path":path,"status":"modified","binary":false}));
        } else if line.starts_with("new file mode") {
            if let Some(file) = files.last_mut() {
                file["status"] = Value::String("added".into());
            }
        } else if line.starts_with("deleted file mode") {
            if let Some(file) = files.last_mut() {
                file["status"] = Value::String("deleted".into());
            }
        } else if line.starts_with("Binary files ") {
            if let Some(file) = files.last_mut() {
                file["binary"] = Value::Bool(true);
            }
        }
    }
    files
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

        fs::remove_dir_all(root).unwrap();
    }
}
