use std::fmt;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::http::{parse_request, HttpRequest, ParseError};

const BUFFER_SIZE: usize = 16 * 1024;

#[derive(Debug)]
pub enum ProxyError {
    Io(std::io::Error),
    Http(ParseError),
    InvalidHost,
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Http(error) => write!(f, "HTTP error: {error}"),
            Self::InvalidHost => f.write_str("missing or invalid Host header"),
        }
    }
}

impl std::error::Error for ProxyError {}

impl From<std::io::Error> for ProxyError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ParseError> for ProxyError {
    fn from(error: ParseError) -> Self {
        Self::Http(error)
    }
}

pub async fn proxy_connection(mut client: TcpStream) -> Result<(), ProxyError> {
    let mut buffer = vec![0_u8; BUFFER_SIZE];
    let bytes_read = client.read(&mut buffer).await?;

    if bytes_read == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let parsed = parse_request(&request)?;
    let upstream = upstream_address(&parsed)?;

    let mut backend = TcpStream::connect(upstream).await?;
    backend.write_all(&buffer[..bytes_read]).await?;
    backend.shutdown().await?;

    let mut response = Vec::new();
    backend.read_to_end(&mut response).await?;
    client.write_all(&response).await?;
    client.shutdown().await?;

    Ok(())
}

fn upstream_address(request: &HttpRequest) -> Result<&str, ProxyError> {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or(ProxyError::InvalidHost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_backend_from_host_header() {
        let request = parse_request("GET / HTTP/1.1\r\nHost: 127.0.0.1:9000\r\n\r\n").unwrap();
        assert_eq!(upstream_address(&request).unwrap(), "127.0.0.1:9000");
    }

    #[test]
    fn host_header_lookup_is_case_insensitive() {
        let request = parse_request("GET / HTTP/1.1\r\nhOsT: 127.0.0.1:9001\r\n\r\n").unwrap();
        assert_eq!(upstream_address(&request).unwrap(), "127.0.0.1:9001");
    }

    #[test]
    fn missing_host_is_rejected() {
        let request = parse_request("GET / HTTP/1.1\r\nConnection: close\r\n\r\n").unwrap();
        assert!(matches!(upstream_address(&request), Err(ProxyError::InvalidHost)));
    }
}
