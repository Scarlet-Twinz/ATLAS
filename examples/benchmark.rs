use std::time::Instant;

use atlas::{parse_request, parse_response_head};

fn main() {
    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n";
    let iterations = 1_000_000u64;

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(parse_request(request).unwrap());
        std::hint::black_box(parse_response_head(response).unwrap());
    }
    let elapsed = start.elapsed();
    let rate = iterations as f64 / elapsed.as_secs_f64();

    println!("ATLAS parser benchmark");
    println!("iterations={iterations}");
    println!("elapsed_ms={:.3}", elapsed.as_secs_f64() * 1000.0);
    println!("iterations_per_second={rate:.0}");
}
