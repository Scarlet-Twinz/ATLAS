use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use atlas::{proxy_connection_with_state, ProxyConfig, ProxyState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn unavailable_backend_returns_503() {
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let backend = unused_local_address();
    let config = ProxyConfig {
        backends: vec![backend],
        connect_timeout: Duration::from_millis(100),
        request_timeout: Duration::from_millis(200),
        max_retries: 0,
    };
    let state = Arc::new(ProxyState::new(config));

    let server_state = Arc::clone(&state);
    let server = tokio::spawn(async move {
        let (stream, _) = proxy_listener.accept().await.unwrap();
        proxy_connection_with_state(stream, (*server_state).clone()).await
    });

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();
    client.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 503 Service Unavailable\r\n"));
    server.await.unwrap().unwrap_err();
}

#[tokio::test]
async fn metrics_endpoint_is_available_without_upstream() {
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let state = ProxyState::new(ProxyConfig::default());

    let server = tokio::spawn(async move {
        let (stream, _) = proxy_listener.accept().await.unwrap();
        proxy_connection_with_state(stream, state).await
    });

    let mut client = tokio::net::TcpStream::connect(proxy_addr).await.unwrap();
    client.write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("atlas_requests_total"));
    server.await.unwrap().unwrap();
}

fn unused_local_address() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap()
}
