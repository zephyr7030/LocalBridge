//! Classifies an administrator command as ordinary or destructive.
//!
//! This is text matching, and text matching is not a security boundary: the
//! policy layer used to rely on it and that was the reason it failed. The
//! difference here is what a miss costs. A missed match means the command runs
//! without a prompt but is still recorded in the ledger; it does not mean a
//! boundary was crossed. Confirmation is a courtesy to the user, not the wall.
//!
//! So the bias is deliberately toward silence. Reading, listing, querying,
//! building, installing and starting things are never flagged. Only operations
//! that cannot be undone, or that weaken this machine's own defences, are.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskCategory {
    BulkDelete,
    DiskFormat,
    RegistryDestruction,
    SecurityControls,
    BootAndRecovery,
    AdministratorAccounts,
}

impl RiskCategory {
    /// A stable code. The user-facing wording lives in the frontend, like every
    /// other piece of copy in this application.
    pub const fn code(self) -> &'static str {
        match self {
            Self::BulkDelete => "bulk_delete",
            Self::DiskFormat => "disk_format",
            Self::RegistryDestruction => "registry_destruction",
            Self::SecurityControls => "security_controls",
            Self::BootAndRecovery => "boot_and_recovery",
            Self::AdministratorAccounts => "administrator_accounts",
        }
    }
}

impl fmt::Display for RiskCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Every pattern is a set of fragments that must all appear. A single-element
/// set is a plain substring match. Fragments are matched against the command
/// padded with spaces, so `" format "` does not fire on `Format-Table`.
type Pattern = &'static [&'static str];

const RULES: &[(RiskCategory, &[Pattern])] = &[
    (
        // A delete only counts when the command does not say what it is
        // deleting: a recursion flag or a wildcard. `del a.txt b.txt` names its
        // targets, so it is ordinary work and stays silent.
        RiskCategory::BulkDelete,
        &[
            &[" del ", " /s"],
            &[" del ", "*"],
            &[" erase ", " /s"],
            &[" erase ", "*"],
            &[" rd ", " /s"],
            &[" rmdir ", " /s"],
            &["remove-item", "-recurse"],
            &["remove-item", "*"],
            &[" ri ", "-recurse"],
            &["get-childitem", "remove-item"],
        ],
    ),
    (
        RiskCategory::DiskFormat,
        &[
            &[" format "],
            &["diskpart"],
            &["clear-disk"],
            &["initialize-disk"],
            &["remove-partition"],
            &["set-partition"],
            &["clear-volume"],
        ],
    ),
    (
        // Writing a registry value is routine administration. Removing keys and
        // restoring whole hives over the live ones is not.
        RiskCategory::RegistryDestruction,
        &[
            &[" reg ", " delete "],
            &[" reg ", " import "],
            &[" reg ", " restore "],
            &["remove-item", "hkey"],
            &["remove-item", "hklm"],
            &["remove-item", "hkcu"],
            &["remove-itemproperty", "hk"],
        ],
    ),
    (
        // Turning this machine's own defences off, whichever way it is spelled.
        RiskCategory::SecurityControls,
        &[
            &["set-mppreference"],
            &["add-mppreference"],
            &["netsh", "advfirewall", "set"],
            &["netsh", "firewall", "set"],
            &["set-executionpolicy"],
            &["manage-bde", "-off"],
            &["disable-computerrestore"],
            &[" stop ", "windefend"],
            &["stop-service", "windefend"],
            &[" delete ", "windefend"],
            &[" stop ", "mpssvc"],
            &["stop-service", "mpssvc"],
            &[" delete ", "mpssvc"],
        ],
    ),
    (
        // Removing the way back: restore points, backups, boot configuration.
        RiskCategory::BootAndRecovery,
        &[
            &["bcdedit", "/set"],
            &["bcdedit", "/delete"],
            &["bcdboot"],
            &["bootrec"],
            &["reagentc", "/disable"],
            &["vssadmin", "delete"],
            &["wbadmin", "delete"],
        ],
    ),
    (
        // An account or a group membership outlives everything else here.
        // Listing accounts is not this; granting one is.
        RiskCategory::AdministratorAccounts,
        &[
            &[" net ", " user ", "/add"],
            &[" net ", " user ", "/delete"],
            &[" net ", " localgroup ", "/add"],
            &[" net ", " localgroup ", "/delete"],
            &["new-localuser"],
            &["remove-localuser"],
            &["add-localgroupmember"],
            &["remove-localgroupmember"],
        ],
    ),
];

/// The categories a command falls into, in the order declared above. Empty
/// means nothing matched, which is the expected result for almost everything.
pub fn classify(command: &str) -> Vec<RiskCategory> {
    let normalized = normalize(command);
    RULES
        .iter()
        .filter(|(_, patterns)| {
            patterns
                .iter()
                .any(|pattern| pattern.iter().all(|fragment| normalized.contains(fragment)))
        })
        .map(|(category, _)| *category)
        .collect()
}

/// Lowercase, collapse every run of whitespace to one space, and pad the ends,
/// so a fragment written as `" /s"` matches a flag at the end of the line and a
/// fragment written as `"del "` does not match `handel`.
fn normalize(command: &str) -> String {
    let mut normalized = String::with_capacity(command.len() + 2);
    normalized.push(' ');
    let mut in_whitespace = false;
    for character in command.chars() {
        if character.is_whitespace() {
            in_whitespace = true;
            continue;
        }
        if in_whitespace {
            normalized.push(' ');
            in_whitespace = false;
        }
        normalized.extend(character.to_lowercase());
    }
    normalized.push(' ');
    normalized
}

#[cfg(test)]
#[path = "risk_tests.rs"]
mod tests;
