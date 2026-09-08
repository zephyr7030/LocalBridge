//! A local, append-only record of what LocalBridge actually did.
//!
//! Two things land here.
//!
//! Administrator commands, verbatim. A tool that can run anything with an
//! administrator token and keeps no record of it cannot be answered when the
//! user asks what happened to their files. Reviewing the command text before
//! execution was never the interesting half; being able to read it afterwards
//! is.
//!
//! And the runtime events that used to go to `eprintln!`. This binary is built
//! with `windows_subsystem = "windows"`, so it has no console: everything
//! written to stderr in a release build reaches nobody. That includes the
//! message the connection worker emits when a request handler panics, which is
//! precisely the event worth keeping.
//!
//! The file is deliberately not part of the diagnostics export. An export is
//! meant to be shareable; a command line is whatever the caller typed into it.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const LOG_FILE_NAME: &str = "localbridge-audit.log";
const ROTATED_FILE_NAME: &str = "localbridge-audit.1.log";
const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RECORDED_COMMAND_BYTES: usize = 8 * 1024;
const MAX_RECORDED_MESSAGE_BYTES: usize = 2 * 1024;

static SINK: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn sink() -> &'static Mutex<Option<PathBuf>> {
    SINK.get_or_init(|| Mutex::new(None))
}

/// Point the audit log at the application data directory. It shares the
/// directory the diagnostics export already writes to, so the existing
/// "open logs" action reveals it without a second place to look.
///
/// Records written before this runs are dropped: there is no correct file to
/// put them in yet, and guessing one is worse than losing them.
pub fn install(app_data_dir: &Path) {
    let directory = app_data_dir.join("diagnostics");
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    let mut guard = sink().lock().unwrap_or_else(PoisonError::into_inner);
    *guard = Some(directory.join(LOG_FILE_NAME));
}

/// A runtime event worth keeping across a restart.
pub fn runtime_event(level: &str, component: &str, message: &str) {
    append(json!({
        "kind": "runtime",
        "level": level,
        "component": component,
        "message": truncate(message, MAX_RECORDED_MESSAGE_BYTES),
    }));
}

/// One administrator operation, recorded whatever its outcome.
///
/// `command` is kept as it was executed. A redacted audit trail cannot answer
/// the question it exists for.
pub fn administrator_command(
    route: &str,
    command: &str,
    workdir: Option<&str>,
    outcome: &str,
    exit_code: Option<u32>,
    duration_ms: u64,
    risk: &[&str],
) {
    append(json!({
        "kind": "administrator_command",
        "route": route,
        "command": truncate(command, MAX_RECORDED_COMMAND_BYTES),
        "workdir": workdir,
        "outcome": outcome,
        "exit_code": exit_code,
        "duration_ms": duration_ms,
        "risk": risk,
    }));
}

fn append(mut entry: Value) {
    let Some(path) = current_path() else {
        return;
    };
    if let Some(object) = entry.as_object_mut() {
        object.insert("timestamp_ms".into(), json!(unix_time_ms()));
    }
    let Ok(mut line) = serde_json::to_vec(&entry) else {
        return;
    };
    line.push(b'\n');
    rotate_if_oversized(&path);
    // Auditing is best effort by construction: it must never be able to fail a
    // user operation, and it must never panic inside a worker thread.
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(&line);
    }
}

fn current_path() -> Option<PathBuf> {
    sink()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn rotate_if_oversized(path: &Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() < MAX_LOG_BYTES {
        return;
    }
    let Some(directory) = path.parent() else {
        return;
    };
    let rotated = directory.join(ROTATED_FILE_NAME);
    let _ = fs::remove_file(&rotated);
    let _ = fs::rename(path, &rotated);
}

/// Truncate on a character boundary so the record stays valid UTF-8 and the
/// truncation is visible rather than silent.
fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[truncated]", &value[..end])
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("localbridge-audit-{name}"));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("temp audit directory");
        directory
    }

    #[test]
    fn truncation_keeps_valid_utf8_and_marks_itself() {
        let value = "管理员命令".repeat(64);
        let truncated = truncate(&value, 10);
        assert!(truncated.ends_with("…[truncated]"));
        assert!(truncated.len() < value.len());
    }

    #[test]
    fn short_values_are_recorded_verbatim() {
        assert_eq!(truncate("whoami /all", 64), "whoami /all");
    }

    #[test]
    fn an_uninstalled_sink_drops_records_instead_of_guessing_a_path() {
        // No install() has run in this test, so there is no directory to write
        // to. The call must be a no-op rather than an error or a panic.
        administrator_command("shell", "whoami", None, "completed", Some(0), 1, &[]);
    }

    #[test]
    fn oversized_logs_rotate_and_keep_exactly_one_previous_file() {
        let directory = temp_dir("rotation");
        let path = directory.join(LOG_FILE_NAME);
        fs::write(&path, vec![b'x'; usize::try_from(MAX_LOG_BYTES).unwrap()])
            .expect("oversized log");
        rotate_if_oversized(&path);
        assert!(!path.exists(), "oversized log is moved aside");
        assert!(directory.join(ROTATED_FILE_NAME).exists());
        let _ = fs::remove_dir_all(&directory);
    }
}
