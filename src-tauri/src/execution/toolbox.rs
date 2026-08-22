use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::runtime::run_bounded_command;

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

#[derive(Debug, Clone)]
pub(crate) struct ToolboxResolver {
    bin_dir: PathBuf,
    system32_dir: Option<PathBuf>,
    aria2c: Availability,
    seven_zip: Availability,
    jq: Availability,
    curl: Availability,
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
    fn child_path_exposes_trusted_tools_without_rewriting_shell_text() {
        let resolver = ready();
        let path = resolver.child_path();
        assert!(path.starts_with(r"C:\LocalBridge\runtime\toolbox\bin;C:\Windows\System32"));
        let discovery = resolver.discovery();
        for tool in ["aria2c", "7z", "jq", "curl"] {
            assert_eq!(discovery[tool]["status"], "ready");
        }
    }
}
