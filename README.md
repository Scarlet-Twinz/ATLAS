# ATLAS

**HTTP reverse proxy built in Rust.**

ATLAS is a systems-oriented networking project focused on HTTP request handling, concurrent connections, backend routing, health checks, timeouts, and proxy execution.

## Current milestone

The first milestone establishes the network foundation:

- Rust project structure
- Tokio-based asynchronous runtime
- TCP listener
- Concurrent connection handling
- Initial HTTP/1.1 response generation
- Request-line inspection

## Architecture

```text
Client
  |
  | HTTP
  v
+-------+
| ATLAS |
| Proxy |
+---+---+
    |
    v
 Backend services
```

## Getting started

### Prerequisites

- Rust toolchain with Cargo

Verify the installation:

```bash
rustc --version
cargo --version
```

### Run

```bash
cargo run
```

ATLAS listens on `127.0.0.1:8080` in the current milestone.

### Test

```bash
cargo test
```

## Roadmap

The project will be developed incrementally:

1. HTTP/1.1 request parsing
2. Backend forwarding
3. Keep-alive connections
4. Connection management
5. Multiple backend routing
6. Load balancing
7. Health checking
8. Request timeouts
9. Retry policies
10. Graceful shutdown
11. Access logging and metrics
12. Benchmarks and fault testing

## Author

**Anthony Emmanuella Mmasinachi**

Software developer focused on systems engineering, backend infrastructure, APIs, distributed systems, databases, automation, and practical software architecture.

**GitHub:** https://github.com/Scarlet-Twinz

## License

MIT
