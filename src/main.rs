use std::error::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const LISTEN_ADDR: &str = "127.0.0.1:8080";
const BUFFER_SIZE: usize = 16 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct HttpRequestLine<'a> {
    method: &'a str,
    target: &'a str,
    version: &'a str,
}

fn parse_request_line(request: &str) -> Result<HttpRequestLine<'_>, &'static str> {
    let line = request
        .lines()
        .next()
        .ok_or("missing HTTP request line")?;

    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or("missing HTTP method")?;
    let target = parts.next().ok_or("missing request target")?;
    let version = parts.next().ok_or("missing HTTP version")?;

    if parts.next().is_some() {
        return Err("malformed HTTP request line");
    }

    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err("unsupported HTTP version");
    }

    Ok(HttpRequestLine {
        method,
        target,
        version,
    })
}

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

    match parse_request_line(&request) {
        Ok(request_line) => {
            println!(
                "request: {} {} {}",
                request_line.method, request_line.target, request_line.version
            );
        }
        Err(error) => {
            println!("invalid request: {error}");
            let response = "HTTP/1.1 400 Bad Request\r\nContent-Length: 11\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nbad request\n";
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
            return Ok(());
        }
    }

    let body = "ATLAS is alive\n";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );

    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_http11_request_line() {
        let request = parse_request_line("GET /health HTTP/1.1\r\nHost: localhost\r\n").unwrap();

        assert_eq!(
            request,
            HttpRequestLine {
                method: "GET",
                target: "/health",
                version: "HTTP/1.1",
            }
        );
    }

    #[test]
    fn parses_http10_request_line() {
        let request = parse_request_line("GET / HTTP/1.0\r\n").unwrap();

        assert_eq!(request.version, "HTTP/1.0");
    }

    #[test]
    fn rejects_extra_request_line_fields() {
        assert_eq!(
            parse_request_line("GET / HTTP/1.1 unexpected\r\n"),
            Err("malformed HTTP request line")
        );
    }

    #[test]
    fn rejects_unsupported_http_version() {
        assert_eq!(
            parse_request_line("GET / HTTP/2.0\r\n"),
            Err("unsupported HTTP version")
        );
    }

    #[test]
    fn rejects_empty_request() {
        assert_eq!(parse_request_line(""), Err("missing HTTP request line"));
    }
}
