# ATLAS

**Systems-oriented HTTP reverse proxy built in Rust.**

ATLAS is a focused networking implementation that makes HTTP/1.x framing, TCP connection ownership, upstream routing, retries, health state, and failure handling explicit in the codebase.

It is deliberately not a framework exercise. The project uses Tokio TCP primitives so the important infrastructure behavior remains visible and testable.

## Request Path

```text
Client
  │ HTTP/1.x
  ▼
┌─────────────────────┐
│        ATLAS        │
│ parse → route →     │
│ forward → frame →   │
│ reuse/retry         │
└──────────┬──────────┘
           │
     healthy backend pool
       ┌───┼───┐
       ▼   ▼   ▼
     API-A API-B API-C
```

## Protocol Handling

ATLAS implements explicit HTTP/1.x message framing for:

- request parsing and validation;
- HTTP/1.0 and HTTP/1.1 upstream responses;
- `Content-Length` bodies;
- chunked transfer decoding;
- no-body responses;
- close-delimited responses;
- conflicting `Content-Length` validation;
- unsupported transfer-encoding handling;
- header-size limits; and
- bounded request bodies.

The response framing decision also determines whether an upstream TCP connection is safe to reuse.

## Connection & Routing

- Round-robin selection across configured backends
- Healthy/unhealthy backend tracking
- Upstream TCP connection reuse
- Client keep-alive
- Connect and request timeouts
- Configurable retry attempts
- Passive backend cooldown
- Active TCP health checks
- Graceful shutdown

## Observability

ATLAS exposes a Prometheus-compatible `/metrics` endpoint and structured access logs for request status, upstream failures, timeouts, and bytes returned.

Example:

```text
access method=GET target=/ status=200 bytes=669
```

## Resilience Model

ATLAS distinguishes infrastructure failures from normal application responses.

A failed connection or timed-out upstream can trigger retry/cooldown behavior. A successfully framed response can return its connection to the reuse pool. Close-delimited responses are consumed through connection close and are never incorrectly reused.

Chunked responses are fully consumed before reuse, including buffered body data that arrived with the response headers.

## Verification

The repository contains coverage for:

- HTTP parsing and framing;
- asynchronous runtime behavior;
- connection reuse;
- backend routing and health;
- retries and unavailable upstreams;
- chunked responses;
- metrics;
- fault injection; and
- parser benchmarking.

CI enforces formatting, Clippy with warnings denied, and the test suite.

## Configuration

```text
ATLAS_BACKENDS=127.0.0.1:9000,127.0.0.1:9001
ATLAS_CONNECT_TIMEOUT_MS=2000
ATLAS_REQUEST_TIMEOUT_MS=10000
ATLAS_MAX_RETRIES=2
```

The listener defaults to `127.0.0.1:8080`.

## Local Run

### Clone

```bash
git clone https://github.com/Scarlet-Twinz/ATLAS.git
cd ATLAS
```

Prerequisites:

- Rust/Cargo
- Python 3 for the optional local upstream

Start an upstream:

```bash
python -m http.server 9000
```

Run ATLAS:

```bash
cargo run
```

Test the proxy:

```bash
curl -v http://127.0.0.1:8080/
curl http://127.0.0.1:8080/metrics
```

Validation:

```bash
cargo fmt -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```

## Repository Structure

```text
ATLAS/
├── examples/benchmark.rs
├── tests/fault_injection.rs
├── src/
│   ├── http.rs
│   ├── lib.rs
│   ├── main.rs
│   └── proxy.rs
├── .github/workflows/ci.yml
└── Cargo.toml
```

## Current State

**Complete for the current scope.**

ATLAS has progressed beyond basic forwarding into a focused reverse-proxy implementation with explicit protocol framing, connection reuse, multi-backend routing, retries, timeouts, active health checks, passive failure handling, graceful shutdown, metrics, structured logs, benchmarks, and fault-injection coverage.

Potential next phases include HTTP/2, TLS termination, dynamic configuration, distributed tracing, higher-volume load testing, and a larger connection-pool subsystem. These are intentionally outside the current scope.

## License

MIT

## Author

**Anthony Emmanuella Mmasinachi**

Full-stack and systems engineer focused on backend infrastructure, networking, distributed systems, databases, AI integration, and systems programming.

## Project Links

- **Repository:** https://github.com/Scarlet-Twinz/ATLAS
- **Author:** Anthony Emmanuella Mmasinachi
- **GitHub:** https://github.com/Scarlet-Twinz
