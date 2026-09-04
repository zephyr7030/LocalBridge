use std::sync::Arc;

use crate::credentials::SecretString;
use crate::security::random_hex;

const AUTHORIZATION_PREFIX: &str = "Bearer ";

#[derive(Clone)]
pub(crate) enum ClientAuthenticator {
    Bearer(Arc<SecretString>),
    #[cfg(test)]
    DisabledForIsolatedUnitTest,
}

impl ClientAuthenticator {
    pub(crate) fn generated() -> Result<Self, ()> {
        let bearer = random_hex(32).map_err(|_| ())?;
        let secret = SecretString::new(bearer).map_err(|_| ())?;
        Ok(Self::Bearer(Arc::new(secret)))
    }

    #[cfg(test)]
    pub(crate) const fn disabled_for_isolated_unit_test() -> Self {
        Self::DisabledForIsolatedUnitTest
    }

    pub(crate) fn authenticate(&self, authorization: Option<&str>) -> bool {
        match self {
            Self::Bearer(expected) => authorization
                .and_then(|value| value.strip_prefix(AUTHORIZATION_PREFIX))
                .is_some_and(|provided| constant_time_eq(provided, expected.expose_secret())),
            #[cfg(test)]
            Self::DisabledForIsolatedUnitTest => true,
        }
    }

    pub(crate) fn bearer_copy(&self) -> Option<SecretString> {
        match self {
            Self::Bearer(secret) => SecretString::new(secret.expose_secret()).ok(),
            #[cfg(test)]
            Self::DisabledForIsolatedUnitTest => None,
        }
    }

    #[cfg(debug_assertions)]
    pub(crate) fn test_authorization_header(&self) -> Option<String> {
        match self {
            Self::Bearer(secret) => {
                Some(format!("{AUTHORIZATION_PREFIX}{}", secret.expose_secret()))
            }
            #[cfg(test)]
            Self::DisabledForIsolatedUnitTest => None,
        }
    }
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    let width = left.len().max(right.len());
    for index in 0..width {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

impl std::fmt::Debug for ClientAuthenticator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bearer(_) => formatter.write_str("ClientAuthenticator::Bearer([REDACTED])"),
            #[cfg(test)]
            Self::DisabledForIsolatedUnitTest => {
                formatter.write_str("ClientAuthenticator::DisabledForIsolatedUnitTest")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_authentication_is_exact_and_secret_is_redacted() {
        let authenticator = ClientAuthenticator::generated().unwrap();
        let header = authenticator.test_authorization_header().unwrap();
        assert!(authenticator.authenticate(Some(&header)));
        assert!(!authenticator.authenticate(None));
        assert!(!authenticator.authenticate(Some("Bearer wrong")));
        assert!(!format!("{authenticator:?}").contains(&header));
    }
}
