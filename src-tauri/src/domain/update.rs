use std::fmt;

use semver::Version;
use serde::{Deserialize, Serialize};

use super::OperationError;

const GITHUB_REPOSITORY_MAX_BYTES: usize = 200;
pub const OFFICIAL_GITHUB_REPOSITORY: &str = "zephyr7030/LocalBridge";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProductVersion(Version);

impl ProductVersion {
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("Cargo package version must be valid semver")
    }

    pub fn parse(value: &str) -> Result<Self, ProductVersionError> {
        let normalized = value
            .trim()
            .strip_prefix('v')
            .or_else(|| value.trim().strip_prefix('V'))
            .unwrap_or(value.trim());
        Version::parse(normalized)
            .map(Self)
            .map_err(|_| ProductVersionError::Invalid)
    }
}

impl fmt::Display for ProductVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductVersionError {
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GitHubRepository(String);

impl GitHubRepository {
    pub fn new(value: impl Into<String>) -> Result<Self, GitHubRepositoryError> {
        let value = value.into();
        if value.is_empty() || value.len() > GITHUB_REPOSITORY_MAX_BYTES {
            return Err(GitHubRepositoryError::Invalid);
        }
        let mut parts = value.split('/');
        let Some(owner) = parts.next() else {
            return Err(GitHubRepositoryError::Invalid);
        };
        let Some(repository) = parts.next() else {
            return Err(GitHubRepositoryError::Invalid);
        };
        if parts.next().is_some()
            || !valid_repository_component(owner)
            || !valid_repository_component(repository)
        {
            return Err(GitHubRepositoryError::Invalid);
        }
        Ok(Self(value))
    }

    pub fn official() -> Self {
        Self::new(OFFICIAL_GITHUB_REPOSITORY)
            .expect("the compile-time official GitHub repository must be valid")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn latest_api_url(&self) -> String {
        format!("https://api.github.com/repos/{}/releases/latest", self.0)
    }

    pub fn releases_url(&self) -> String {
        format!("https://github.com/{}/releases", self.0)
    }

    pub fn owns_release_url(&self, value: &str) -> bool {
        value == self.releases_url()
            || value.starts_with(&format!("https://github.com/{}/releases/", self.0))
    }
}

fn valid_repository_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && value != "."
        && value != ".."
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubRepositoryError {
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseDiscovery {
    pub version: ProductVersion,
    pub release_url: String,
}

impl ReleaseDiscovery {
    pub fn new(
        repository: &GitHubRepository,
        version: ProductVersion,
        release_url: impl Into<String>,
    ) -> Result<Self, ReleaseDiscoveryError> {
        let release_url = release_url.into();
        if !repository.owns_release_url(&release_url) {
            return Err(ReleaseDiscoveryError::ForeignReleaseUrl);
        }
        Ok(Self {
            version,
            release_url,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseDiscoveryError {
    ForeignReleaseUrl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCheckTrigger {
    Startup,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateLifecycle {
    SourceUnavailable {
        current_version: ProductVersion,
        reason: String,
    },
    Idle {
        current_version: ProductVersion,
        releases_url: String,
    },
    Checking {
        current_version: ProductVersion,
        releases_url: String,
        operation_id: String,
        trigger: UpdateCheckTrigger,
        attempt: u8,
        started_at_ms: u64,
    },
    Current {
        current_version: ProductVersion,
        releases_url: String,
        operation_id: String,
        checked_at_ms: u64,
    },
    Available {
        current_version: ProductVersion,
        latest_version: ProductVersion,
        release_url: String,
        operation_id: String,
        checked_at_ms: u64,
    },
    Failed {
        current_version: ProductVersion,
        releases_url: String,
        operation_id: String,
        attempts: u8,
        checked_at_ms: u64,
        error: OperationError,
    },
}

impl UpdateLifecycle {
    pub fn current_version(&self) -> &ProductVersion {
        match self {
            Self::SourceUnavailable {
                current_version, ..
            }
            | Self::Idle {
                current_version, ..
            }
            | Self::Checking {
                current_version, ..
            }
            | Self::Current {
                current_version, ..
            }
            | Self::Available {
                current_version, ..
            }
            | Self::Failed {
                current_version, ..
            } => current_version,
        }
    }

    pub fn release_url(&self) -> Option<&str> {
        match self {
            Self::SourceUnavailable { .. } => None,
            Self::Idle { releases_url, .. }
            | Self::Checking { releases_url, .. }
            | Self::Current { releases_url, .. }
            | Self::Failed { releases_url, .. } => Some(releases_url),
            Self::Available { release_url, .. } => Some(release_url),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_versions_are_semantic_and_release_tags_may_use_v_prefix() {
        assert!(
            ProductVersion::parse("v1.10.0").unwrap() > ProductVersion::parse("1.9.9").unwrap()
        );
        assert!(
            ProductVersion::parse("1.0.0").unwrap() > ProductVersion::parse("1.0.0-rc.1").unwrap()
        );
        assert!(ProductVersion::parse("1.0").is_err());
    }

    #[test]
    fn github_repository_and_release_links_are_exactly_scoped() {
        let repository = GitHubRepository::new("owner/LocalBridge").unwrap();
        assert_eq!(
            repository.latest_api_url(),
            "https://api.github.com/repos/owner/LocalBridge/releases/latest"
        );
        assert!(
            repository.owns_release_url("https://github.com/owner/LocalBridge/releases/tag/v1.2.3")
        );
        assert!(!repository.owns_release_url("https://example.invalid/releases/v1.2.3"));
        assert!(GitHubRepository::new("owner/repo/extra").is_err());
        assert_eq!(
            GitHubRepository::official().as_str(),
            OFFICIAL_GITHUB_REPOSITORY
        );
        assert_eq!(
            GitHubRepository::official().releases_url(),
            "https://github.com/zephyr7030/LocalBridge/releases"
        );
    }
}
