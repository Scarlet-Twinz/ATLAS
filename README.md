# ATLAS

**Systems-oriented HTTP reverse proxy built in Rust.**

ATLAS is a compact networking infrastructure project built around explicit HTTP/1.x framing, TCP connection management, upstream routing, resilience, observability, and failure handling. Rather than hiding proxy behavior behind a high-level framework, ATLAS keeps the core request, response, connection, routing, and recovery paths visible in the codebase.

## Architecture

```text
                         +------------------+
Client ----------------> |      ATLAS       |
HTTP/1.x                 |  Reverse Proxy   |
                         +--------+---------+
                                  |
                         +--------+---------+
                         |    Backend Pool   |
                         +---+------+-----+--+
                             |      |     |
                             v      v     v
                           API-A  API-B  API-C
```

The runtime accepts client connections, parses HTTP requests, selects an upstream backend, forwards requests, interprets upstream response framing, and returns the response while maintaining backend health and connection state.

## Core capabilities

### HTTP and protocol handling

- HTTP/1.1 request parsing and validation
- HTTP/1.0 and HTTP/1.1 upstream response parsing
- Content-Length response framing
- Chunked transfer decoding
- No-body response handling
- Close-delimited response handling
- Header-size enforcement
- Request-body forwarding with a bounded 16 MiB limit
- Validation of conflicting Content-Length headers
- Explicit handling of unsupported transfer encodings

### Upstream and connection management

- Configurable round-robin backend pool
- Multiple upstream backends
- Upstream TCP connection reuse for reusable responses
- Client-side keep-alive
- Explicit connection-reuse decisions based on HTTP framing
- Connect and request timeouts
- Configurable retry attempts

### Resilience

- Passive backend failure cooldown
- Active TCP backend health checks
- Healthy/unhealthy backend selection
- Retry handling for upstream failures
- Timeout handling without leaving stalled requests behind
- Graceful Ctrl+C shutdown
- Bounded request and response processing

### Observability

- Prometheus-compatible `/metrics` endpoint
- Runtime request counters
- Upstream failure and timeout counters
- Bytes-returned counter
- Structured key/value access logs

Example:

```text
access method=GET target=/ status=200 bytes=669
```

### Verification

- Unit tests for HTTP parsing and framing
- Asynchronous runtime tests
- Connection-reuse tests
- Backend health and routing tests
- Chunked-response runtime coverage
- Unavailable-upstream fault injection
- Metrics endpoint coverage
- Parser benchmark example
- Rust formatting and Clippy enforcement

## Configuration

ATLAS can be configured through environment variables:

```text
ATLAS_BACKENDS=127.0.0.1:9000,127.0.0.1:9001
ATLAS_CONNECT_TIMEOUT_MS=2000
ATLAS_REQUEST_TIMEOUT_MS=10000
ATLAS_MAX_RETRIES=2
```

If `ATLAS_BACKENDS` is not provided, ATLAS uses `127.0.0.1:9000`.

The listener binds to `127.0.0.1:8080`.

## Getting started

### Prerequisites

- Rust toolchain with Cargo
- Python 3 for the optional local HTTP upstream used in the example below

Verify Rust:

```bash
rustc --version
cargo --version
```

### Start an upstream

For a quick local upstream:

```bash
python -m http.server 9000
```

### Start ATLAS

```bash
cargo run
```

ATLAS starts on `127.0.0.1:8080` and routes traffic to the configured backend pool.

### Query through the proxy

```bash
curl -v http://127.0.0.1:8080/
```

### Inspect metrics

```bash
curl http://127.0.0.1:8080/metrics
```

## Testing

Run the full validation set:

```bash
cargo fmt -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```

Run the parser benchmark:

```bash
cargo run --example benchmark --release
```

The benchmark executes one million parser iterations and is intended as a lightweight regression signal rather than a production load benchmark.

## Resilience model

ATLAS distinguishes between transient upstream failures and normal application-level failures. Failed upstream operations can be retried according to `ATLAS_MAX_RETRIES`, while failed backends enter a short cooldown to reduce repeated traffic to an unhealthy endpoint.

Active health checks probe configured backends so recovered services can return to rotation. Backend selection is round-robin across currently healthy endpoints.

Connection reuse is driven by HTTP message framing. Reusable HTTP/1.x responses can keep an upstream TCP connection alive when their body boundary is explicit. Close-delimited responses are consumed through connection close and are never returned to the reuse pool.

Chunked responses are fully consumed before an upstream connection is considered reusable, including data that may already have been buffered together with the response headers.

## Engineering approach

ATLAS is intentionally implemented on Tokio TCP primitives rather than a high-level reverse-proxy framework. This keeps the important infrastructure behavior explicit:

- socket lifecycle and connection ownership;
- HTTP request and response framing;
- backend selection;
- upstream connection reuse;
- retry and timeout boundaries;
- health state;
- failure classification;
- runtime metrics and access logging.

The project is designed as a focused systems-engineering implementation rather than a feature-heavy application. Its purpose is to demonstrate control over networking fundamentals, protocol boundaries, concurrency, resilience, and operational behavior.

## Repository structure

```text
ATLAS/
├── .github/
│   └── workflows/
│       └── ci.yml
├── examples/
│   └── benchmark.rs
├── tests/
│   └── fault_injection.rs
├── src/
│   ├── http.rs
│   ├── lib.rs
│   ├── main.rs
│   └── proxy.rs
├── Cargo.toml
├── Cargo.lock
└── README.md
```

## Current state

**ATLAS is complete for its current scope.** The implementation has progressed beyond basic request forwarding into a focused reverse-proxy system with HTTP framing, connection reuse, multi-backend routing, retries, timeouts, active health checks, passive failure handling, graceful shutdown, metrics, structured logging, benchmarks, and fault-injection coverage.

The repository's CI workflow enforces formatting, Clippy with warnings denied, and the test suite on pushes and pull requests to `main`.

Potential future work such as HTTP/2, TLS termination, dynamic configuration, distributed tracing, high-volume load testing, or a larger connection-pool subsystem would represent a new engineering phase rather than unfinished core work.

## Author

**Anthony Emmanuella Mmasinachi**

Software developer focused on systems engineering, backend infrastructure, APIs, distributed systems, databases, automation, and practical software architecture.

**GitHub:** https://github.com/Scarlet-Twinz

## License

MIT
