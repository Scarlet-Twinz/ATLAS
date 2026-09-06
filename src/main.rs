use std::error::Error;
use tokio::net::TcpListener;

use atlas::proxy_connection;

const LISTEN_ADDR: &str = "127.0.0.1:8080";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(LISTEN_ADDR).await?;
    println!("ATLAS listening on http://{LISTEN_ADDR}");

    loop {
        let (stream, peer) = listener.accept().await?;
        println!("accepted connection from {peer}");

        tokio::spawn(async move {
            if let Err(error) = proxy_connection(stream).await {
                eprintln!("connection error from {peer}: {error}");
            }
        });
    }
}
