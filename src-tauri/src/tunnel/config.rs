use std::path::PathBuf;

use super::fault::TunnelError;

const OPENAI_CONTROL_PLANE: &str = "https://api.openai.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelId(String);

impl TunnelId {
    pub fn new(value: impl Into<String>) -> Result<Self, TunnelError> {
        let value = value.into();
        let suffix = value
            .strip_prefix("tunnel_")
            .ok_or(TunnelError::InvalidTunnelId)?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(TunnelError::InvalidTunnelId);
        }
        Ok(Self(value))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelRuntimeConfig {
    pub install_root: PathBuf,
    pub health_state_dir: PathBuf,
    pub tunnel_id: TunnelId,
    pub mcp_guard_port: u16,
    control_plane_base_url: String,
    #[cfg(test)]
    embedded_mcp_stub: bool,
}

impl TunnelRuntimeConfig {
    pub fn new(
        install_root: impl Into<PathBuf>,
        health_state_dir: impl Into<PathBuf>,
        tunnel_id: TunnelId,
        mcp_guard_port: u16,
    ) -> Result<Self, TunnelError> {
        let install_root = install_root.into();
        if install_root.as_os_str().is_empty() {
            return Err(TunnelError::InvalidInstallRoot);
        }
        if mcp_guard_port == 0 {
            return Err(TunnelError::InvalidMcpTarget);
        }
        let health_state_dir = health_state_dir.into();
        if health_state_dir.as_os_str().is_empty() {
            return Err(TunnelError::InvalidHealthStateDirectory);
        }
        Ok(Self {
            install_root,
            health_state_dir,
            tunnel_id,
            mcp_guard_port,
            control_plane_base_url: OPENAI_CONTROL_PLANE.to_string(),
            #[cfg(test)]
            embedded_mcp_stub: false,
        })
    }

    pub(crate) fn mcp_target(&self) -> String {
        format!(
            "url=http://127.0.0.1:{}/mcp,channel=main",
            self.mcp_guard_port
        )
    }

    pub(crate) fn control_plane_base_url(&self) -> &str {
        &self.control_plane_base_url
    }

    #[cfg(test)]
    pub(crate) fn with_test_embedded_mcp_stub(mut self) -> Self {
        self.embedded_mcp_stub = true;
        self
    }

    #[cfg(test)]
    pub(crate) fn embedded_mcp_stub(&self) -> bool {
        self.embedded_mcp_stub
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn with_test_control_plane_base_url(mut self, value: &str) -> Result<Self, TunnelError> {
        let port = value
            .strip_prefix("http://127.0.0.1:")
            .and_then(|rest| rest.parse::<u16>().ok())
            .filter(|port| *port != 0)
            .ok_or(TunnelError::InvalidControlPlaneOverride)?;
        let _ = port;
        self.control_plane_base_url = value.to_string();
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_id() -> TunnelId {
        TunnelId::new("tunnel_0123456789abcdef0123456789abcdef").unwrap()
    }

    #[test]
    fn tunnel_id_is_exact_lowercase_hex_contract() {
        assert!(TunnelId::new("tunnel_0123456789abcdef0123456789abcdef").is_ok());
        for invalid in [
            "0123456789abcdef0123456789abcdef",
            "tunnel_0123456789ABCDEF0123456789ABCDEF",
            "tunnel_0123456789abcdef",
            "tunnel_0123456789abcdef0123456789abcdef00",
            "tunnel_0123456789abcdef0123456789abcdeg",
        ] {
            assert!(matches!(
                TunnelId::new(invalid),
                Err(TunnelError::InvalidTunnelId)
            ));
        }
    }

    #[test]
    fn config_never_uses_relative_empty_install_root_or_non_loopback_test_control_plane() {
        assert!(matches!(
            TunnelRuntimeConfig::new("", "health", valid_id(), 1234),
            Err(TunnelError::InvalidInstallRoot)
        ));
        assert!(matches!(
            TunnelRuntimeConfig::new("root", "health", valid_id(), 0),
            Err(TunnelError::InvalidMcpTarget)
        ));
        let config = TunnelRuntimeConfig::new("root", "health", valid_id(), 1234).unwrap();
        assert_eq!(config.control_plane_base_url(), OPENAI_CONTROL_PLANE);
        assert_eq!(
            config.mcp_target(),
            "url=http://127.0.0.1:1234/mcp,channel=main"
        );
        #[cfg(debug_assertions)]
        {
            assert!(
                config
                    .clone()
                    .with_test_control_plane_base_url("http://127.0.0.1:4321")
                    .is_ok()
            );
            for invalid in [
                "http://0.0.0.0:4321",
                "http://localhost:4321",
                "http://127.0.0.1:0",
                "http://127.0.0.1:4321/path",
                "https://127.0.0.1:4321",
            ] {
                assert!(matches!(
                    config.clone().with_test_control_plane_base_url(invalid),
                    Err(TunnelError::InvalidControlPlaneOverride)
                ));
            }
        }
    }
}
