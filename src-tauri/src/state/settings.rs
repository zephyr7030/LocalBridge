use super::PermissionMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub permission_mode: PermissionMode,
    pub auto_start_services: bool,
    pub onboarding_complete: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            permission_mode: PermissionMode::Edit,
            auto_start_services: false,
            onboarding_complete: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_domain_contains_preferences_but_no_persistence_or_secret_material() {
        let settings = Settings::default();
        assert_eq!(settings.permission_mode, PermissionMode::Edit);
        assert!(!settings.auto_start_services);
        assert!(!settings.onboarding_complete);
    }
}
