use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::runtime::run_bounded_command;

use super::shell::ResolvedShellKind;

const ARIA2C_SHA256: &str = "be2099c214f63a3cb4954b09a0becd6e2e34660b886d4c898d260febfe9d70c2";
const SEVEN_ZIP_SHA256: &str = "35d4d69d7cd6cb44558f208c3b1334268013f9daf82d2dda848893a1c30c59c2";
const JQ_SHA256: &str = "a6fc67fedaf9128a3309a1e2ebb8b986aeccf70122ee46d2cb4849e423f0c627";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Availability {
    Ready,
    Missing,
    IntegrityMismatch,
    CapabilityMissing,
}

impl Availability {
    const fn name(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Missing => "missing",
            Self::IntegrityMismatch => "integrity_mismatch",
            Self::CapabilityMissing => "capability_missing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolboxErrorKind {
    RuntimeUnavailable,
    CapabilityUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolboxError {
    pub(crate) kind: ToolboxErrorKind,
    pub(crate) tool: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolboxResolver {
    bin_dir: PathBuf,
    system32_dir: Option<PathBuf>,
    aria2c: Availability,
    seven_zip: Availability,
    jq: Availability,
    curl: Availability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolboxTool {
    Aria2c,
    SevenZip,
    Jq,
    Curl,
}

impl ToolboxTool {
    const fn name(self) -> &'static str {
        match self {
            Self::Aria2c => "aria2c",
            Self::SevenZip => "7z",
            Self::Jq => "jq",
            Self::Curl => "curl",
        }
    }
}

impl ToolboxResolver {
    pub(crate) fn probe(install_root: &Path) -> Self {
        let packaged_bin = install_root.join("runtime").join("toolbox").join("bin");
        #[cfg(debug_assertions)]
        let bin_dir = if packaged_bin.is_dir() {
            packaged_bin
        } else {
            install_root
                .join("src-tauri")
                .join("target")
                .join("toolbox-stage")
                .join("bin")
        };
        #[cfg(not(debug_assertions))]
        let bin_dir = packaged_bin;
        let system32_dir = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .map(|path| path.join("System32"));
        let curl_path = system32_dir.as_ref().map(|path| path.join("curl.exe"));
        Self {
            aria2c: bundled_availability(&bin_dir.join("aria2c.exe"), ARIA2C_SHA256),
            seven_zip: bundled_availability(&bin_dir.join("7z.exe"), SEVEN_ZIP_SHA256),
            jq: bundled_availability(&bin_dir.join("jq.exe"), JQ_SHA256),
            curl: curl_path
                .as_deref()
                .map(curl_availability)
                .unwrap_or(Availability::Missing),
            bin_dir,
            system32_dir,
        }
    }

    pub(crate) fn discovery(&self) -> Value {
        json!({
            "aria2c":{"version":"1.37.0","status":self.aria2c.name(),"bundled":true},
            "7z":{"version":"26.02","status":self.seven_zip.name(),"bundled":true},
            "jq":{"version":"1.8.2","status":self.jq.name(),"bundled":true},
            "curl":{"status":self.curl.name(),"bundled":false,"source":"windows_system32"},
            "runtime_download":false,
            "persistent_path_mutation":false
        })
    }

    pub(crate) fn child_path(&self) -> String {
        let mut value = self.bin_dir.to_string_lossy().into_owned();
        if let Some(system32) = &self.system32_dir {
            value.push(';');
            value.push_str(&system32.to_string_lossy());
        }
        if let Some(path) = std::env::var_os("PATH").filter(|path| !path.is_empty()) {
            value.push(';');
            value.push_str(&path.to_string_lossy());
        }
        value
    }

    pub(crate) fn rewrite_command(
        &self,
        kind: ResolvedShellKind,
        command: &str,
    ) -> Result<String, ToolboxError> {
        let mut output = String::with_capacity(command.len());
        let mut cursor = 0usize;
        let mut command_target = true;
        let mut quote = None;
        let mut escaped = false;
        while cursor < command.len() {
            let ch = command[cursor..]
                .chars()
                .next()
                .expect("cursor is on a character boundary");
            if command_target {
                if ch.is_whitespace() {
                    output.push(ch);
                    cursor += ch.len_utf8();
                    continue;
                }
                if is_separator(ch) {
                    output.push(ch);
                    cursor += ch.len_utf8();
                    command_target = true;
                    continue;
                }
                let end = token_end(command, cursor);
                let original = &command[cursor..end];
                let token = unquote_token(original);
                if let Some(tool) = toolbox_tool(token) {
                    let path = self.executable(tool)?;
                    output.push_str(&shell_executable(kind, &path));
                } else {
                    output.push_str(original);
                }
                cursor = end;
                command_target = false;
                continue;
            }
            output.push(ch);
            cursor += ch.len_utf8();
            if escaped {
                escaped = false;
                continue;
            }
            match kind {
                ResolvedShellKind::Cmd => {
                    if ch == '^' {
                        escaped = true;
                    } else if ch == '"' {
                        quote = if quote == Some('"') { None } else { Some('"') };
                    } else if quote.is_none() && is_separator(ch) {
                        command_target = true;
                    }
                }
                ResolvedShellKind::PowerShellCore | ResolvedShellKind::WindowsPowerShell => {
                    if quote == Some('"') && ch == '`' {
                        escaped = true;
                    } else if quote == Some(ch) && matches!(ch, '\'' | '"') {
                        quote = None;
                    } else if quote.is_none() && matches!(ch, '\'' | '"') {
                        quote = Some(ch);
                    } else if quote.is_none() && ch == '`' {
                        escaped = true;
                    } else if quote.is_none() && is_separator(ch) {
                        command_target = true;
                    }
                }
            }
        }
        Ok(output)
    }

    fn executable(&self, tool: ToolboxTool) -> Result<PathBuf, ToolboxError> {
        let availability = match tool {
            ToolboxTool::Aria2c => self.aria2c,
            ToolboxTool::SevenZip => self.seven_zip,
            ToolboxTool::Jq => self.jq,
            ToolboxTool::Curl => self.curl,
        };
        if availability != Availability::Ready {
            return Err(ToolboxError {
                kind: if availability == Availability::CapabilityMissing {
                    ToolboxErrorKind::CapabilityUnavailable
                } else {
                    ToolboxErrorKind::RuntimeUnavailable
                },
                tool: tool.name(),
            });
        }
        Ok(match tool {
            ToolboxTool::Aria2c => self.bin_dir.join("aria2c.exe"),
            ToolboxTool::SevenZip => self.bin_dir.join("7z.exe"),
            ToolboxTool::Jq => self.bin_dir.join("jq.exe"),
            ToolboxTool::Curl => self
                .system32_dir
                .as_ref()
                .expect("ready curl has System32 path")
                .join("curl.exe"),
        })
    }
}

fn bundled_availability(path: &Path, expected: &str) -> Availability {
    if !path.is_file() {
        return Availability::Missing;
    }
    match file_sha256(path) {
        Ok(actual) if actual == expected => Availability::Ready,
        _ => Availability::IntegrityMismatch,
    }
}

fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn curl_availability(path: &Path) -> Availability {
    if !path.is_file() {
        return Availability::Missing;
    }
    let Some(cwd) = path.parent() else {
        return Availability::CapabilityMissing;
    };
    let args = [OsString::from("--version")];
    let Ok(result) = run_bounded_command(path, &args, cwd, Duration::from_secs(2), 32 * 1024)
    else {
        return Availability::CapabilityMissing;
    };
    if result.exit_code != 0 || result.timed_out || result.truncated {
        return Availability::CapabilityMissing;
    }
    let output = String::from_utf8_lossy(&result.output).to_ascii_lowercase();
    if output.contains("protocols:") && output.contains("http") && output.contains("https") {
        Availability::Ready
    } else {
        Availability::CapabilityMissing
    }
}

fn toolbox_tool(token: &str) -> Option<ToolboxTool> {
    let token = token.trim();
    if token.eq_ignore_ascii_case("aria2c") || token.eq_ignore_ascii_case("aria2c.exe") {
        Some(ToolboxTool::Aria2c)
    } else if token.eq_ignore_ascii_case("7z") || token.eq_ignore_ascii_case("7z.exe") {
        Some(ToolboxTool::SevenZip)
    } else if token.eq_ignore_ascii_case("jq") || token.eq_ignore_ascii_case("jq.exe") {
        Some(ToolboxTool::Jq)
    } else if token.eq_ignore_ascii_case("curl") || token.eq_ignore_ascii_case("curl.exe") {
        Some(ToolboxTool::Curl)
    } else {
        None
    }
}

fn token_end(command: &str, start: usize) -> usize {
    let mut quote = None;
    for (offset, ch) in command[start..].char_indices() {
        if let Some(expected) = quote {
            if ch == expected {
                quote = None;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
        } else if offset > 0 && (ch.is_whitespace() || is_separator(ch)) {
            return start + offset;
        }
    }
    command.len()
}

fn unquote_token(token: &str) -> &str {
    if token.len() >= 2 {
        let first = token.as_bytes()[0];
        let last = token.as_bytes()[token.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &token[1..token.len() - 1];
        }
    }
    token
}

const fn is_separator(ch: char) -> bool {
    matches!(ch, '&' | '|' | ';' | '\r' | '\n' | '(' | ')' | '{' | '}')
}

fn shell_executable(kind: ResolvedShellKind, path: &Path) -> String {
    match kind {
        ResolvedShellKind::Cmd => format!("\"{}\"", path.to_string_lossy()),
        ResolvedShellKind::PowerShellCore | ResolvedShellKind::WindowsPowerShell => {
            format!("& '{}'", path.to_string_lossy().replace('\'', "''"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> ToolboxResolver {
        ToolboxResolver {
            bin_dir: PathBuf::from(r"C:\LocalBridge\runtime\toolbox\bin"),
            system32_dir: Some(PathBuf::from(r"C:\Windows\System32")),
            aria2c: Availability::Ready,
            seven_zip: Availability::Ready,
            jq: Availability::Ready,
            curl: Availability::Ready,
        }
    }

    #[test]
    fn trusted_targets_are_rewritten_without_touching_inert_arguments() {
        let resolver = ready();
        let cmd = resolver
            .rewrite_command(
                ResolvedShellKind::Cmd,
                "echo jq && jq . a.json | aria2c.exe --version",
            )
            .unwrap();
        assert!(cmd.contains("echo jq"));
        assert!(cmd.contains(r#""C:\LocalBridge\runtime\toolbox\bin\jq.exe""#));
        assert!(cmd.contains(r#""C:\LocalBridge\runtime\toolbox\bin\aria2c.exe""#));
        let powershell = resolver
            .rewrite_command(
                ResolvedShellKind::WindowsPowerShell,
                "curl --version | jq .",
            )
            .unwrap();
        assert!(powershell.starts_with(r"& 'C:\Windows\System32\curl.exe'"));
        assert!(powershell.contains(r"| & 'C:\LocalBridge\runtime\toolbox\bin\jq.exe'"));
    }

    #[test]
    fn quoted_toolbox_names_after_literal_separators_remain_inert() {
        let resolver = ready();
        let cmd = resolver
            .rewrite_command(
                ResolvedShellKind::Cmd,
                r#"echo "jq | aria2c & 7z ; curl" && 7z --help"#,
            )
            .unwrap();
        assert!(cmd.contains(r#""jq | aria2c & 7z ; curl""#));
        assert!(cmd.ends_with(r#"&& "C:\LocalBridge\runtime\toolbox\bin\7z.exe" --help"#));

        let powershell = resolver
            .rewrite_command(
                ResolvedShellKind::WindowsPowerShell,
                "Write-Output 'jq | aria2c & 7z ; curl'; jq .",
            )
            .unwrap();
        assert!(powershell.contains("'jq | aria2c & 7z ; curl'"));
        assert!(powershell.ends_with(r"; & 'C:\LocalBridge\runtime\toolbox\bin\jq.exe' ."));
    }

    #[test]
    fn curl_capability_failure_is_typed_and_bundled_missing_is_runtime_unavailable() {
        let mut resolver = ready();
        resolver.curl = Availability::CapabilityMissing;
        let curl = resolver
            .rewrite_command(ResolvedShellKind::Cmd, "curl --version")
            .unwrap_err();
        assert_eq!(curl.kind, ToolboxErrorKind::CapabilityUnavailable);
        resolver.aria2c = Availability::Missing;
        let aria = resolver
            .rewrite_command(ResolvedShellKind::Cmd, "aria2c.exe --version")
            .unwrap_err();
        assert_eq!(aria.kind, ToolboxErrorKind::RuntimeUnavailable);
    }
}
