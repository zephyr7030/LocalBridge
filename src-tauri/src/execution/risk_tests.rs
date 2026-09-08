use super::*;

fn categories(command: &str) -> Vec<&'static str> {
    classify(command)
        .into_iter()
        .map(RiskCategory::code)
        .collect()
}

#[test]
fn ordinary_administrator_work_is_not_flagged() {
    // A prompt that fires on everything is a prompt nobody reads, so the cases
    // that must stay silent are the ones worth pinning.
    for command in [
        "whoami /all",
        "sc query wuauserv",
        "sc stop wuauserv",
        "Get-Service wuauserv",
        "Stop-Service Spooler",
        "reg query HKLM\\SOFTWARE\\Microsoft",
        "reg add HKLM\\SOFTWARE\\Contoso /v Mode /d fast /f",
        "Get-ChildItem C:\\Windows\\System32 -Filter *.dll",
        "cargo build --release",
        "npm ci",
        "Format-Table -AutoSize",
        "net user",
        "Get-LocalUser",
        "icacls C:\\work",
        "takeown /f C:\\work\\locked.txt",
        "schtasks /query",
        "shutdown /r /t 0",
        "curl -o tool.exe https://example.invalid/tool.exe",
        "bcdedit /enum",
        "handel /s",
    ] {
        assert!(
            categories(command).is_empty(),
            "{command} -> {:?}",
            categories(command)
        );
    }
}

#[test]
fn a_delete_that_names_its_targets_is_ordinary_work() {
    // The command says exactly what disappears, so there is nothing for a
    // confirmation dialog to add that the command text does not already say.
    for command in [
        "del C:\\work\\a.txt C:\\work\\b.txt",
        "Remove-Item C:\\work\\build.log",
        "rd C:\\work\\empty",
    ] {
        assert!(categories(command).is_empty(), "{command}");
    }
}

#[test]
fn a_delete_that_does_not_name_its_targets_is_flagged() {
    for command in [
        "del C:\\data /s /q",
        "del C:\\data\\*.*",
        "Remove-Item C:\\data -Recurse -Force",
        "Remove-Item C:\\data\\*",
        "rd C:\\data /s /q",
        "Get-ChildItem C:\\data | Remove-Item",
    ] {
        assert!(categories(command).contains(&"bulk_delete"), "{command}");
    }
}

#[test]
fn irreversible_operations_are_flagged_by_category() {
    for (command, expected) in [
        ("format D: /y", "disk_format"),
        ("diskpart /s script.txt", "disk_format"),
        ("reg delete HKLM\\SYSTEM\\Foo /f", "registry_destruction"),
        (
            "reg restore HKLM\\SYSTEM backup.hiv",
            "registry_destruction",
        ),
        (
            "Set-MpPreference -DisableRealtimeMonitoring $true",
            "security_controls",
        ),
        (
            "Set-ExecutionPolicy Bypass -Scope Process",
            "security_controls",
        ),
        ("sc stop WinDefend", "security_controls"),
        ("bcdedit /set safeboot minimal", "boot_and_recovery"),
        ("vssadmin delete shadows /all", "boot_and_recovery"),
        (
            "net localgroup administrators someone /add",
            "administrator_accounts",
        ),
        ("New-LocalUser backdoor", "administrator_accounts"),
    ] {
        assert!(
            categories(command).contains(&expected),
            "{command} should be {expected}, got {:?}",
            categories(command)
        );
    }
}

#[test]
fn matching_ignores_case_and_irregular_spacing() {
    assert!(categories("DEL    C:\\data    /S").contains(&"bulk_delete"));
    assert!(categories("remove-item\tC:\\data\t-Recurse").contains(&"bulk_delete"));
}

#[test]
fn a_command_can_belong_to_several_categories() {
    let found = categories("reg delete HKLM\\SYSTEM\\Foo /f & vssadmin delete shadows /all");
    assert!(found.contains(&"registry_destruction"));
    assert!(found.contains(&"boot_and_recovery"));
}
