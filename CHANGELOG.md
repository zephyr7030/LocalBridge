# Changelog

All notable user-visible changes to LocalBridge are recorded here.

## [Unreleased]

No user-visible changes yet.

## [0.1.1] - 2026-08-19

### Added
- Added the LocalBridge-owned `filesystem` MCP tool with exactly `list`, `stat`, `read`, `write`, `search`, `copy`, `move`, `delete`, and `hash` actions.

### Fixed
- Prevented LocalBridge-managed runtime, background, and managed command process trees from creating visible console windows during normal GUI operation.

### Security
- Unified structured filesystem path authority across Edit, Full, and Elevated modes, including active-workspace confinement, Broker-only administrator routing, reparse-point protection, final-path revalidation, bounded operations, atomic writes, and verified cross-volume moves.

> Do not use raw commit history as release notes.
