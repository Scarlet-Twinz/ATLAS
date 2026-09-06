use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use atlas::{proxy_connection_with_state, ProxyConfig, ProxyState};
use tokio::net::TcpListener;
use tokio::time::interval;

const LISTEN_ADDR: &str = "127.0.0.1:8080";
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = ProxyConfig::from_env()?;
    let state = ProxyState::new(config.clone());
    let metrics = state.metrics();
    let state = Arc::new(state);

    let listener = TcpListener::bind(LISTEN_ADDR).await?;
    println!("ATLAS listening on http://{LISTEN_ADDR}");
    println!("upstreams={:?}", config.backends);
    println!("connect_timeout_ms={}", config.connect_timeout.as_millis());
    println!("request_timeout_ms={}", config.request_timeout.as_millis());
    println!("max_retries={}", config.max_retries);

    let metrics_task = {
        let metrics = metrics.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(30));
            loop {
                ticker.tick().await;
                println!("--- ATLAS metrics ---\n{}", metrics.render_prometheus());
            }
        })
    };

    let health_task = {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let mut ticker = interval(HEALTH_CHECK_INTERVAL);
            loop {
                ticker.tick().await;
                for backend in state.backends().to_vec() {
                    let healthy = state.health_check(backend).await;
                    println!("health backend={backend} healthy={healthy}");
                }
            }
        })
    };

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, peer) = result?;
                let state = Arc::clone(&state);
                println!("accepted connection from {peer}");
                tokio::spawn(async move {
                    if let Err(error) = proxy_connection_with_state(stream, (*state).clone()).await {
                        eprintln!("connection error from {peer}: {error}");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => {
                println!("shutdown signal received; stopping ATLAS");
                break;
            }
        }
    }

    health_task.abort();
    metrics_task.abort();
    Ok(())
}
