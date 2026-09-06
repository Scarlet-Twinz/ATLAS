use std::error::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const LISTEN_ADDR: &str = "127.0.0.1:8080";
const BUFFER_SIZE: usize = 16 * 1024;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(LISTEN_ADDR).await?;
    println!("ATLAS listening on http://{LISTEN_ADDR}");

    loop {
        let (stream, peer) = listener.accept().await?;
        println!("accepted connection from {peer}");

        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream).await {
                eprintln!("connection error from {peer}: {error}");
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut buffer = vec![0_u8; BUFFER_SIZE];
    let bytes_read = stream.read(&mut buffer).await?;

    if bytes_read == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let request_line = request.lines().next().unwrap_or("");

    println!("request: {request_line}");

    let response = "HTTP/1.1 200 OK\r\nContent-Length: 14\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nATLAS is alive\n";
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;

    Ok(())
}
