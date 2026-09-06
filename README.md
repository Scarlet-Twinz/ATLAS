# ATLAS

**HTTP reverse proxy built in Rust.**

ATLAS is a systems-oriented networking project focused on explicit HTTP/TCP behavior, upstream routing, connection management, resilience, observability, and failure handling.

## Architecture

```text
                         +------------------+
Client ----------------> |      ATLAS       |
HTTP/1.1                 |  Reverse Proxy   |
                         +--------+---------+
                                  |
                         +--------+---------+
                         |    Backend Pool   |
                         +---+------+-----+--+
                             |      |     |
                             v      v     v
                           API-A  API-B  API-C
```

## Implemented

- Tokio asynchronous TCP runtime
- HTTP/1.1 request parsing and validation
- HTTP/1.0 and HTTP/1.1 upstream response parsing
- Content-Length response framing
- Chunked transfer decoding
- No-body response handling
- Close-delimited response handling
- Request body forwarding up to 16 MiB
- Configurable round-robin backend pool
- Upstream connection reuse for reusable HTTP responses
- Client-side keep-alive
- Connect and request timeouts
- Configurable retry attempts
- Passive backend failure cooldown
- Active backend health checks
- Graceful Ctrl+C shutdown
- Prometheus-compatible `/metrics` endpoint
- Runtime Prometheus counters
- Structured access logs
- Fault-injection integration tests
- Parser benchmark example
- Unit and asynchronous runtime tests

## Configuration

```text
ATLAS_BACKENDS=127.0.0.1:9000,127.0.0.1:9001
ATLAS_CONNECT_TIMEOUT_MS=2000
ATLAS_REQUEST_TIMEOUT_MS=10000
ATLAS_MAX_RETRIES=2
```

If `ATLAS_BACKENDS` is not provided, ATLAS uses `127.0.0.1:9000`.

The listener binds to `127.0.0.1:8080`.

## Running

### Start an upstream

For a quick local upstream:

```bash
python -m http.server 9000
```

### Start ATLAS

```bash
cargo run
```

### Query through the proxy

```bash
curl -v http://127.0.0.1:8080/
```

### Metrics

```bash
curl http://127.0.0.1:8080/metrics
```

### Tests

```bash
cargo fmt -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```

### Parser benchmark

```bash
cargo run --example benchmark --release
```

The benchmark measures one million request/response parser iterations. It is intended as a lightweight regression signal rather than a production load benchmark.

## Resilience

ATLAS retries failed upstream operations according to `ATLAS_MAX_RETRIES`. Failed backends enter a short cooldown and active health checks periodically probe configured backends so recovered services can return to rotation.

Reusable HTTP/1.1 responses with explicit framing can keep the upstream TCP connection alive. HTTP/1.0 responses require explicit `Connection: keep-alive`. Close-delimited responses are never reused.

## Metrics and logging

`/metrics` exposes Prometheus text for total requests, successful requests, failed requests, upstream failures, upstream timeouts, and bytes returned to clients.

Access logs use stable key/value fields:

```text
access method=GET target=/ status=200 bytes=669
```

## Testing

The test suite covers parsing, framing, routing, backend state, connection reuse decisions, runtime response handling, chunked responses, unavailable-upstream behavior, and the metrics endpoint.

The fault-injection tests deliberately use an unavailable local backend to verify that ATLAS returns `503 Service Unavailable` instead of hanging or crashing.

## Engineering notes

ATLAS is intentionally built around Tokio TCP primitives rather than a high-level reverse-proxy framework. Connection handling, HTTP framing, backend selection, retries, timeouts, health state, and observability remain explicit in the codebase.

The implementation deliberately keeps close-delimited responses non-reusable because their message boundary is the TCP connection close. Chunked responses are fully consumed before a reusable upstream connection is returned to the pool.

## Repository structure

```text
ATLAS/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── examples/
│   └── benchmark.rs
├── tests/
│   └── fault_injection.rs
└── src/
    ├── http.rs
    ├── lib.rs
    ├── main.rs
    └── proxy.rs
```

## Current state

**ATLAS is complete for its current scope.** The project has progressed from a basic forwarding prototype into a compact reverse-proxy implementation with HTTP framing, upstream reuse, routing, retries, timeouts, active health checks, metrics, structured logging, benchmarks, and fault-injection coverage.

Further work such as HTTP/2, TLS termination, dynamic configuration, a larger connection pool, distributed tracing, or high-volume load testing would be a new project phase rather than required finishing work for this version.

## Author

**Anthony Emmanuella Mmasinachi**

Software developer focused on systems engineering, backend infrastructure, APIs, distributed systems, databases, automation, and practical software architecture.

**GitHub:** https://github.com/Scarlet-Twinz

## License

MIT
