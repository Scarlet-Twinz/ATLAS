# ATLAS

**HTTP reverse proxy built in Rust.**

ATLAS is a systems-oriented networking project focused on HTTP/1.1 parsing, asynchronous TCP handling, upstream forwarding, backend pools, retries, timeouts, passive health tracking, request bodies, graceful shutdown, and Prometheus-style metrics.

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

The proxy accepts HTTP/1.1 connections, parses the request headers, reads a declared request body, selects an available upstream from a round-robin pool, forwards the request, and returns the upstream response.

## Implemented

- Tokio asynchronous runtime
- TCP listener with one task per client connection
- HTTP/1.1 request-line and header parser
- Header-size enforcement
- Duplicate-header detection
- Configurable upstream backend pool
- Deterministic round-robin backend selection
- Passive backend health tracking after failures
- Automatic backend cooldown and recovery
- Connect timeout
- Request/upstream response timeout
- Configurable retry attempts
- Request `Content-Length` handling
- Request body forwarding up to 16 MiB
- HTTP keep-alive request loop
- `Connection: close` handling
- 503/504/400 proxy error responses
- Graceful shutdown on Ctrl+C
- Prometheus-style runtime counters
- Unit tests for parser, routing, backend state, configuration, and metrics

## Configuration

ATLAS can be configured with environment variables.

```text
ATLAS_BACKENDS=127.0.0.1:9000,127.0.0.1:9001
ATLAS_CONNECT_TIMEOUT_MS=2000
ATLAS_REQUEST_TIMEOUT_MS=10000
ATLAS_MAX_RETRIES=2
```

If `ATLAS_BACKENDS` is not provided, ATLAS uses `127.0.0.1:9000`.

The listener currently binds to `127.0.0.1:8080`.

## Running

### Prerequisites

- Rust toolchain with Cargo

Verify the installation:

```bash
rustc --version
cargo --version
```

### Start ATLAS

```bash
cargo run
```

With multiple upstreams on Windows PowerShell:

```powershell
$env:ATLAS_BACKENDS="127.0.0.1:9000,127.0.0.1:9001"
cargo run
```

### Test

```bash
cargo test
```

### Format and lint

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Example request path

```text
Client
  |
  | GET / HTTP/1.1
  v
ATLAS :8080
  |
  | round-robin selection
  v
Backend :9000
  |
  | HTTP response
  v
ATLAS
  |
  v
Client
```

A second backend can be added through `ATLAS_BACKENDS`; ATLAS rotates selections and temporarily removes a backend after an upstream connection or timeout failure.

## Metrics

ATLAS maintains counters for:

- total requests
- successful requests
- failed requests
- upstream failures
- upstream timeouts
- bytes returned to clients

The metrics are currently rendered as Prometheus-style text in the runtime log every 30 seconds. An HTTP `/metrics` endpoint is intentionally not added yet.

## Engineering notes

ATLAS is intentionally implemented around Tokio TCP primitives rather than hiding proxy behavior behind a high-level reverse-proxy framework. The project therefore keeps connection handling, request framing, backend selection, timeout policy, retries, and failure tracking explicit.

The current upstream response path waits for the upstream connection to close after ATLAS finishes sending the request. This keeps the implementation deterministic while leaving room for a later response-framing layer that can reuse upstream connections safely.

Health tracking is passive: a failed upstream is cooled down for five seconds and can be selected again afterward. There is no active background health-check protocol yet.

## Repository structure

```text
ATLAS/
├── Cargo.toml
├── README.md
└── src/
    ├── http.rs
    ├── lib.rs
    ├── main.rs
    └── proxy.rs
```

## Current state

ATLAS has moved beyond the initial single-backend forwarding prototype into a configurable reverse-proxy core with backend selection, retries, timeout enforcement, request-body support, connection reuse at the client side, passive failure handling, shutdown control, and runtime metrics.

The next natural engineering layers are response framing, upstream connection reuse, active health checks, a dedicated metrics endpoint, structured access logs, benchmarks, and fault-injection tests.

## Author

**Anthony Emmanuella Mmasinachi**

Software developer focused on systems engineering, backend infrastructure, APIs, distributed systems, databases, automation, and practical software architecture.

**GitHub:** https://github.com/Scarlet-Twinz

## License

MIT
