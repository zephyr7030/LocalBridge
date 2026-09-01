use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::time::Duration;

use serde::Deserialize;

use super::fault::{TunnelError, classify_control_plane_error};

const MAX_RESPONSE: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
struct AdminStatus {
    tunnel_metadata: Option<TunnelMetadata>,
    tunnel_metadata_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TunnelMetadata {
    #[serde(default)]
    mcp_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorEndpoint(String);

impl ConnectorEndpoint {
    fn from_verified_metadata(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.len() > 2048
            || !value.starts_with("https://")
            || value.chars().any(char::is_whitespace)
            || value.chars().any(char::is_control)
        {
            return None;
        }
        let authority = value
            .strip_prefix("https://")?
            .split(['/', '?', '#'])
            .next()?;
        if authority.is_empty() || authority.contains('@') {
            return None;
        }
        Some(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadyProbe {
    pub(crate) ready: bool,
    pub(crate) connector_endpoint: Option<ConnectorEndpoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HealthEndpoint {
    port: u16,
}

impl HealthEndpoint {
    pub(crate) fn parse(value: &str) -> Result<Self, TunnelError> {
        let port = value
            .trim()
            .trim_end_matches('/')
            .strip_prefix("http://127.0.0.1:")
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|port| *port != 0)
            .ok_or(TunnelError::HealthUrlInvalid)?;
        Ok(Self { port })
    }

    #[cfg(test)]
    pub(crate) fn probe_ready(&self) -> Result<bool, TunnelError> {
        Ok(self.probe_ready_metadata()?.ready)
    }

    pub(crate) fn probe_ready_metadata(&self) -> Result<ReadyProbe, TunnelError> {
        if get(self.port, "/readyz")?.status != 200 {
            return Ok(ReadyProbe {
                ready: false,
                connector_endpoint: None,
            });
        }
        let status = get(self.port, "/api/status")?;
        if status.status != 200 {
            return Err(TunnelError::HealthProtocol);
        }
        parse_admin_status(&status.body)
    }

    pub(crate) fn probe_ready_metadata_with_timeout(
        &self,
        transport_timeout: Duration,
    ) -> Result<ReadyProbe, TunnelError> {
        if get_with_timeouts(
            self.port,
            "/readyz",
            transport_timeout,
            transport_timeout,
            Some(transport_timeout),
        )?
        .status
            != 200
        {
            return Ok(ReadyProbe {
                ready: false,
                connector_endpoint: None,
            });
        }
        let status = get_with_timeouts(
            self.port,
            "/api/status",
            transport_timeout,
            transport_timeout,
            Some(transport_timeout),
        )?;
        if status.status != 200 {
            return Err(TunnelError::HealthProtocol);
        }
        parse_admin_status(&status.body)
    }
}

fn parse_admin_status(body: &[u8]) -> Result<ReadyProbe, TunnelError> {
    let admin: AdminStatus =
        serde_json::from_slice(body).map_err(|_| TunnelError::HealthProtocol)?;
    if let Some(error) = admin
        .tunnel_metadata_error
        .filter(|value| !value.trim().is_empty())
    {
        return Err(TunnelError::ControlPlane(classify_control_plane_error(
            &error,
        )));
    }
    let Some(metadata) = admin.tunnel_metadata else {
        return Ok(ReadyProbe {
            ready: false,
            connector_endpoint: None,
        });
    };
    let connector_endpoint = metadata
        .mcp_url
        .as_deref()
        .and_then(ConnectorEndpoint::from_verified_metadata);
    Ok(ReadyProbe {
        ready: true,
        connector_endpoint,
    })
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn get(port: u16, path: &str) -> Result<HttpResponse, TunnelError> {
    get_with_timeouts(
        port,
        path,
        Duration::from_millis(500),
        Duration::from_secs(2),
        None,
    )
}

fn get_with_timeouts(
    port: u16,
    path: &str,
    connect_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Option<Duration>,
) -> Result<HttpResponse, TunnelError> {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let mut stream = TcpStream::connect_timeout(&address.into(), connect_timeout)
        .map_err(|_| TunnelError::HealthUnavailable)?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|_| TunnelError::HealthUnavailable)?;
    if let Some(write_timeout) = write_timeout {
        stream
            .set_write_timeout(Some(write_timeout))
            .map_err(|_| TunnelError::HealthUnavailable)?;
    }
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|_| TunnelError::HealthUnavailable)?;
    let mut bytes = Vec::new();
    stream
        .take((MAX_RESPONSE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| TunnelError::HealthUnavailable)?;
    if bytes.len() > MAX_RESPONSE {
        return Err(TunnelError::HealthProtocol);
    }
    parse_response(&bytes)
}

fn parse_response(bytes: &[u8]) -> Result<HttpResponse, TunnelError> {
    let split = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(TunnelError::HealthProtocol)?;
    let headers = std::str::from_utf8(&bytes[..split]).map_err(|_| TunnelError::HealthProtocol)?;
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(TunnelError::HealthProtocol)?;
    let mut chunked = false;
    let mut content_length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Transfer-Encoding")
                && value.trim().eq_ignore_ascii_case("chunked")
            {
                chunked = true;
            }
            if name.eq_ignore_ascii_case("Content-Length") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let raw = &bytes[(split + 4)..];
    let body = if chunked {
        decode_chunked(raw)?
    } else {
        raw.to_vec()
    };
    if let Some(expected) = content_length {
        if expected != body.len() {
            return Err(TunnelError::HealthProtocol);
        }
    }
    Ok(HttpResponse { status, body })
}

fn decode_chunked(mut input: &[u8]) -> Result<Vec<u8>, TunnelError> {
    let mut output = Vec::new();
    loop {
        let line_end = input
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or(TunnelError::HealthProtocol)?;
        let size_text =
            std::str::from_utf8(&input[..line_end]).map_err(|_| TunnelError::HealthProtocol)?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or(""), 16)
            .map_err(|_| TunnelError::HealthProtocol)?;
        input = &input[(line_end + 2)..];
        if size == 0 {
            break;
        }
        if input.len() < size + 2 || &input[size..(size + 2)] != b"\r\n" {
            return Err(TunnelError::HealthProtocol);
        }
        output.extend_from_slice(&input[..size]);
        input = &input[(size + 2)..];
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    fn fake_health(status_body: &'static str) -> (HealthEndpoint, JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let count = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..count]);
                let body = if request.starts_with("GET /api/status ") {
                    status_body
                } else {
                    ""
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (HealthEndpoint { port }, handle)
    }

    #[test]
    fn ready_requires_control_plane_metadata_not_readyz_alone() {
        let (health, server) = fake_health(
            r#"{"tunnel_metadata":null,"tunnel_metadata_error":"dial tcp 127.0.0.1:9: connection refused"}"#,
        );
        assert!(matches!(
            health.probe_ready(),
            Err(TunnelError::ControlPlane(
                super::super::fault::ControlPlaneFault::Network
            ))
        ));
        server.join().unwrap();
    }

    #[test]
    fn authenticated_metadata_without_error_satisfies_lb008_ready_evidence() {
        let (health, server) = fake_health(
            r#"{"tunnel_metadata":{"tunnel_id":"synthetic"},"tunnel_metadata_error":null}"#,
        );
        assert!(health.probe_ready().unwrap());
        server.join().unwrap();
    }

    #[test]
    fn connector_endpoint_comes_only_from_explicit_valid_https_metadata() {
        let explicit = parse_admin_status(
            br#"{"tunnel_metadata":{"tunnel_id":"synthetic","mcp_url":"https://example.openai.test/v1/mcp/synthetic"},"tunnel_metadata_error":null}"#,
        )
        .unwrap();
        assert!(explicit.ready);
        assert_eq!(
            explicit.connector_endpoint.unwrap().as_str(),
            "https://example.openai.test/v1/mcp/synthetic"
        );

        for body in [
            br#"{"tunnel_metadata":{"tunnel_id":"synthetic","mcp_url_path":"/v1/mcp/synthetic"},"tunnel_metadata_error":null}"#.as_slice(),
            br#"{"tunnel_metadata":{"tunnel_id":"synthetic","mcp_url":"http://example.test/v1/mcp/synthetic"},"tunnel_metadata_error":null}"#.as_slice(),
            br#"{"tunnel_metadata":{"tunnel_id":"synthetic","mcp_url":"https://user@example.test/v1/mcp/synthetic"},"tunnel_metadata_error":null}"#.as_slice(),
        ] {
            let parsed = parse_admin_status(body).unwrap();
            assert!(parsed.ready);
            assert!(parsed.connector_endpoint.is_none());
        }
    }

    #[test]
    fn authentication_error_is_typed_non_recoverable() {
        let (health, server) = fake_health(
            r#"{"tunnel_metadata":null,"tunnel_metadata_error":"401 unauthorized invalid api key"}"#,
        );
        let error = health.probe_ready().unwrap_err();
        assert!(matches!(
            error,
            TunnelError::ControlPlane(super::super::fault::ControlPlaneFault::Authentication)
        ));
        assert_eq!(
            error.retryability(),
            super::super::fault::Retryability::NonRecoverable
        );
        server.join().unwrap();
    }

    #[test]
    fn unavailable_endpoint_is_distinct_from_malformed_protocol() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(matches!(
            HealthEndpoint { port }.probe_ready(),
            Err(TunnelError::HealthUnavailable)
        ));
    }
}
