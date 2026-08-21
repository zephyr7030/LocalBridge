# Changelog

All notable user-visible changes to LocalBridge are recorded here.

## [Unreleased]

No user-visible changes yet.

## [0.1.2] - 2026-08-21

### Improved
- Multiple MCP sessions and app windows now share bounded, fair work scheduling while retaining session-scoped request and cancellation isolation.
- Detached commands and long-running tasks now keep stable identities and converge to explicit terminal outcomes across disconnects and runtime restarts.
- Permission and workspace changes now reconcile desired and observed state before exposing effective authority, preventing partially applied control-plane state.
- The app now renders one revisioned live-state snapshot, including scheduler pressure and actionable faults, without guessing task completion or runtime activity.

### Reliability
- Runtime restart recovery marks orphaned executions as lost and preserves unaffected sessions and tasks.
- Lock contention and unavailable observations are reported as stale or unavailable instead of fabricated running state.

## [0.1.1] - 2026-08-21

### Added
- Added a structured `filesystem` tool for common file operations such as listing, reading, writing, searching, copying, moving, deleting, and hashing files without relying on ad-hoc Shell commands.

### Improved
- Multiple MCP clients can now stay connected at the same time without a new session invalidating an existing one.
- Long-running and interrupted tasks now converge to a reliable terminal state instead of remaining permanently stuck as running or waiting.
- Packaged background runtime, Tunnel, recovery, autostart, and managed command paths run without unwanted visible console windows.
- Windows development command compatibility was improved, including ordinary workspace cleanup such as `rmdir /s /q`.

### Security
- Hardened Elevated filesystem authorization against hard-link, junction/reparse, final-object identity, race, and control-plane alias bypasses.
- Tightened Elevated process/Shell authorization so unreviewable administrator execution cannot bypass LocalBridge control-plane protections.
- Runtime API Key remains in Windows Credential Manager and is removed by the uninstaller.
- Release provenance is now bound to a fresh installer build instead of allowing an older installer to be relabeled as the current source revision.

> Release notes intentionally describe user-visible behavior rather than raw commit history.
