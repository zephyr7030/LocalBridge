use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesystemPathDecision {
    Allowed,
    ProtectedControlPlane,
}

pub struct FilesystemPathPolicy;

impl FilesystemPathPolicy {
    pub fn evaluate(path: &str) -> FilesystemPathDecision {
        let path = Path::new(path);
        let protected = path.components().any(|component| {
            let Component::Normal(component) = component else {
                return false;
            };
            let component = component.to_string_lossy();
            component.eq_ignore_ascii_case("localbridge")
                || component.eq_ignore_ascii_case("com.localbridge.desktop")
                || component.eq_ignore_ascii_case("runtime-policy.toml")
                || component.eq_ignore_ascii_case("runtime-manifest.toml")
                || component.eq_ignore_ascii_case("startup-profile.json")
        });
        if protected {
            FilesystemPathDecision::ProtectedControlPlane
        } else {
            FilesystemPathDecision::Allowed
        }
    }

    pub fn allows(path: &str) -> bool {
        Self::evaluate(path) == FilesystemPathDecision::Allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_is_path_component_based_not_shell_text_classification() {
        assert_eq!(
            FilesystemPathPolicy::evaluate(r"C:\ProgramData\LocalBridge\settings.json"),
            FilesystemPathDecision::ProtectedControlPlane
        );
        assert_eq!(
            FilesystemPathPolicy::evaluate(r"C:\work\echo LocalBridge.txt"),
            FilesystemPathDecision::Allowed
        );
        assert_eq!(
            FilesystemPathPolicy::evaluate(r"C:\work\runtime-policy.toml"),
            FilesystemPathDecision::ProtectedControlPlane
        );
    }
}
