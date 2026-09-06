use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

use crate::http::{parse_request, HttpRequest, ParseError, MAX_HEADER_BYTES};

const BUFFER_SIZE: usize = 16 * 1024;
const BACKEND_COOLDOWN: Duration = Duration::from_secs(5);
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_RETRIES: usize = 2;

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub backends: Vec<SocketAddr>,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_retries: usize,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            backends: vec![SocketAddr::from(([127, 0, 0, 1], 9000))],
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            max_retries: DEFAULT_RETRIES,
        }
    }
}

impl ProxyConfig {
    pub fn from_env() -> Result<Self, ProxyError> {
        let backends = match std::env::var("ATLAS_BACKENDS") {
            Ok(value) => parse_backends(&value)?,
            Err(_) => Self::default().backends,
        };

        if backends.is_empty() {
            return Err(ProxyError::NoBackends);
        }

        let connect_timeout = env_duration("ATLAS_CONNECT_TIMEOUT_MS", DEFAULT_CONNECT_TIMEOUT)?;
        let request_timeout = env_duration("ATLAS_REQUEST_TIMEOUT_MS", DEFAULT_REQUEST_TIMEOUT)?;
        let max_retries = std::env::var("ATLAS_MAX_RETRIES")
            .ok()
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| ProxyError::InvalidConfiguration("ATLAS_MAX_RETRIES"))?
            .unwrap_or(DEFAULT_RETRIES);

        Ok(Self {
            backends,
            connect_timeout,
            request_timeout,
            max_retries,
        })
    }
}

#[derive(Debug, Default)]
pub struct ProxyMetrics {
    requests_total: AtomicU64,
    requests_succeeded: AtomicU64,
    requests_failed: AtomicU64,
    upstream_failures: AtomicU64,
    upstream_timeouts: AtomicU64,
    bytes_to_client: AtomicU64,
}

impl ProxyMetrics {
    fn request_started(&self) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
    }

    fn request_succeeded(&self, bytes: usize) {
        self.requests_succeeded.fetch_add(1, Ordering::Relaxed);
        self.bytes_to_client.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    fn request_failed(&self) {
        self.requests_failed.fetch_add(1, Ordering::Relaxed);
    }

    fn upstream_failure(&self) {
        self.upstream_failures.fetch_add(1, Ordering::Relaxed);
    }

    fn upstream_timeout(&self) {
        self.upstream_timeouts.fetch_add(1, Ordering::Relaxed);
    }

    pub fn render_prometheus(&self) -> String {
        format!(
            "# TYPE atlas_requests_total counter\natlas_requests_total {}\n# TYPE atlas_requests_succeeded counter\natlas_requests_succeeded {}\n# TYPE atlas_requests_failed counter\natlas_requests_failed {}\n# TYPE atlas_upstream_failures counter\natlas_upstream_failures {}\n# TYPE atlas_upstream_timeouts counter\natlas_upstream_timeouts {}\n# TYPE atlas_bytes_to_client counter\natlas_bytes_to_client {}\n",
            self.requests_total.load(Ordering::Relaxed),
            self.requests_succeeded.load(Ordering::Relaxed),
            self.requests_failed.load(Ordering::Relaxed),
            self.upstream_failures.load(Ordering::Relaxed),
            self.upstream_timeouts.load(Ordering::Relaxed),
            self.bytes_to_client.load(Ordering::Relaxed),
        )
    }
}

#[derive(Debug, Clone)]
struct BackendState {
    address: SocketAddr,
    unhealthy_until: Option<Instant>,
}

impl BackendState {
    fn healthy(&self, now: Instant) -> bool {
        self.unhealthy_until.is_none_or(|until| until <= now)
    }
}

#[derive(Debug)]
struct BackendPool {
    backends: Vec<BackendState>,
    next: usize,
}

impl BackendPool {
    fn new(backends: &[SocketAddr]) -> Self {
        Self {
            backends: backends
                .iter()
                .copied()
                .map(|address| BackendState {
                    address,
                    unhealthy_until: None,
                })
                .collect(),
            next: 0,
        }
    }

    fn select(&mut self) -> Option<SocketAddr> {
        let now = Instant::now();
        for _ in 0..self.backends.len() {
            let index = self.next % self.backends.len();
            self.next = (self.next + 1) % self.backends.len();
            if self.backends[index].healthy(now) {
                return Some(self.backends[index].address);
            }
        }
        None
    }

    fn mark_failure(&mut self, address: SocketAddr) {
        if let Some(backend) = self.backends.iter_mut().find(|backend| backend.address == address) {
            backend.unhealthy_until = Some(Instant::now() + BACKEND_COOLDOWN);
        }
    }

    fn mark_success(&mut self, address: SocketAddr) {
        if let Some(backend) = self.backends.iter_mut().find(|backend| backend.address == address) {
            backend.unhealthy_until = None;
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProxyState {
    config: ProxyConfig,
    pool: Arc<Mutex<BackendPool>>,
    metrics: Arc<ProxyMetrics>,
}

impl ProxyState {
    pub fn new(config: ProxyConfig) -> Self {
        Self {
            pool: Arc::new(Mutex::new(BackendPool::new(&config.backends))),
            config,
            metrics: Arc::new(ProxyMetrics::default()),
        }
    }

    pub fn metrics(&self) -> Arc<ProxyMetrics> {
        Arc::clone(&self.metrics)
    }
}

#[derive(Debug)]
pub enum ProxyError {
    Io(std::io::Error),
    Http(ParseError),
    InvalidHost,
    InvalidBackend(String),
    NoBackends,
    NoHealthyBackends,
    UpstreamUnavailable,
    UpstreamTimeout,
    InvalidConfiguration(&'static str),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Http(error) => write!(f, "HTTP error: {error}"),
            Self::InvalidHost => f.write_str("missing or invalid Host header"),
            Self::InvalidBackend(value) => write!(f, "invalid backend address: {value}"),
            Self::NoBackends => f.write_str("no upstream backends configured"),
            Self::NoHealthyBackends => f.write_str("no healthy upstream backends available"),
            Self::UpstreamUnavailable => f.write_str("upstream unavailable"),
            Self::UpstreamTimeout => f.write_str("upstream request timed out"),
            Self::InvalidConfiguration(name) => write!(f, "invalid configuration: {name}"),
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

pub async fn proxy_connection(client: TcpStream) -> Result<(), ProxyError> {
    proxy_connection_with_state(client, ProxyState::new(ProxyConfig::default())).await
}

pub async fn proxy_connection_with_state(mut client: TcpStream, state: ProxyState) -> Result<(), ProxyError> {
    let mut read_buffer = Vec::with_capacity(BUFFER_SIZE);
    let mut chunk = vec![0_u8; BUFFER_SIZE];

    loop {
        let bytes_read = client.read(&mut chunk).await?;
        if bytes_read == 0 {
            return Ok(());
        }

        read_buffer.extend_from_slice(&chunk[..bytes_read]);
        if read_buffer.len() > MAX_HEADER_BYTES {
            state.metrics.request_failed();
            return Err(ProxyError::Http(ParseError::HeadersTooLarge));
        }

        let Some(header_end) = find_header_end(&read_buffer) else {
            continue;
        };

        let request_text = String::from_utf8_lossy(&read_buffer[..header_end]);
        let request = parse_request(&request_text)?;
        state.metrics.request_started();

        let request_bytes = read_buffer[..header_end].to_vec();
        let response = match forward_with_retries(&state, &request, &request_bytes).await {
            Ok(response) => response,
            Err(error) => {
                state.metrics.request_failed();
                write_error_response(&mut client, &error).await?;
                return Err(error);
            }
        };

        client.write_all(&response).await?;
        state.metrics.request_succeeded(response.len());

        read_buffer.drain(..header_end);
        if request_has_connection_close(&request) {
            client.shutdown().await?;
            return Ok(());
        }
    }
}

async fn forward_with_retries(
    state: &ProxyState,
    request: &HttpRequest,
    request_bytes: &[u8],
) -> Result<Vec<u8>, ProxyError> {
    let attempts = state.config.max_retries.saturating_add(1);

    for _ in 0..attempts {
        let backend = {
            let mut pool = state.pool.lock().await;
            pool.select()
        };

        let Some(backend) = backend else {
            return Err(ProxyError::NoHealthyBackends);
        };

        match forward_once(state, backend, request, request_bytes).await {
            Ok(response) => {
                state.pool.lock().await.mark_success(backend);
                return Ok(response);
            }
            Err(ProxyError::UpstreamTimeout) => {
                state.metrics.upstream_timeout();
                state.pool.lock().await.mark_failure(backend);
            }
            Err(ProxyError::Io(_)) | Err(ProxyError::UpstreamUnavailable) => {
                state.metrics.upstream_failure();
                state.pool.lock().await.mark_failure(backend);
            }
            Err(error) => return Err(error),
        }
    }

    Err(ProxyError::UpstreamUnavailable)
}

async fn forward_once(
    state: &ProxyState,
    backend: SocketAddr,
    _request: &HttpRequest,
    request_bytes: &[u8],
) -> Result<Vec<u8>, ProxyError> {
    let connect = timeout(state.config.connect_timeout, TcpStream::connect(backend)).await;
    let mut upstream = match connect {
        Ok(Ok(stream)) => stream,
        Ok(Err(_)) => return Err(ProxyError::UpstreamUnavailable),
        Err(_) => return Err(ProxyError::UpstreamTimeout),
    };

    upstream.set_nodelay(true)?;
    upstream.write_all(request_bytes).await?;
    upstream.shutdown().await?;

    let read_result = timeout(state.config.request_timeout, async {
        let mut response = Vec::new();
        upstream.read_to_end(&mut response).await?;
        Ok::<Vec<u8>, std::io::Error>(response)
    })
    .await;

    match read_result {
        Ok(Ok(response)) if response.is_empty() => Err(ProxyError::UpstreamUnavailable),
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Err(ProxyError::Io(error)),
        Err(_) => Err(ProxyError::UpstreamTimeout),
    }
}

fn request_has_connection_close(request: &HttpRequest) -> bool {
    request.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("connection")
            && value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("close"))
    })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

async fn write_error_response(client: &mut TcpStream, error: &ProxyError) -> Result<(), std::io::Error> {
    let (status, reason) = match error {
        ProxyError::Http(_) => (400, "Bad Request"),
        ProxyError::UpstreamTimeout => (504, "Gateway Timeout"),
        ProxyError::NoHealthyBackends | ProxyError::UpstreamUnavailable => (503, "Service Unavailable"),
        _ => (502, "Bad Gateway"),
    };

    let body = format!("ATLAS {status} {reason}\n");
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: text/plain\r\n\r\n{body}",
        body.len()
    );

    client.write_all(response.as_bytes()).await
}

fn parse_backends(value: &str) -> Result<Vec<SocketAddr>, ProxyError> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.parse().map_err(|_| ProxyError::InvalidBackend(value.to_owned())))
        .collect()
}

fn env_duration(name: &str, default: Duration) -> Result<Duration, ProxyError> {
    let Some(value) = std::env::var(name).ok() else {
        return Ok(default);
    };

    let millis = value
        .parse::<u64>()
        .map_err(|_| ProxyError::InvalidConfiguration(Box::leak(name.to_owned().into_boxed_str())))?;
    Ok(Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_backends_round_robin() {
        let addresses = vec![
            SocketAddr::from(([127, 0, 0, 1], 9000)),
            SocketAddr::from(([127, 0, 0, 1], 9001)),
        ];
        let mut pool = BackendPool::new(&addresses);

        assert_eq!(pool.select(), Some(addresses[0]));
        assert_eq!(pool.select(), Some(addresses[1]));
        assert_eq!(pool.select(), Some(addresses[0]));
    }

    #[test]
    fn failed_backend_is_temporarily_removed() {
        let addresses = vec![SocketAddr::from(([127, 0, 0, 1], 9000))];
        let mut pool = BackendPool::new(&addresses);
        pool.mark_failure(addresses[0]);
        assert_eq!(pool.select(), None);
    }

    #[test]
    fn successful_backend_is_restored() {
        let addresses = vec![SocketAddr::from(([127, 0, 0, 1], 9000))];
        let mut pool = BackendPool::new(&addresses);
        pool.mark_failure(addresses[0]);
        pool.mark_success(addresses[0]);
        assert_eq!(pool.select(), Some(addresses[0]));
    }

    #[test]
    fn parses_backend_list() {
        let backends = parse_backends("127.0.0.1:9000, 127.0.0.1:9001").unwrap();
        assert_eq!(backends.len(), 2);
    }
}
