[English](README.md) | [日本語](README.ja.md)

# reverse-proxy

An HTTP reverse proxy written in Rust. Supports multiple backends per route, two load balancing algorithms, periodic health checks, and graceful shutdown.

## Table of Contents

1. [Features](#features)
2. [Design](#design)
3. [Configuration](#configuration)
4. [Getting Started](#getting-started)
5. [Testing](#testing)
6. [CI](#ci)

## Features

- **Load balancing algorithms**: Round robin or least connections, configurable per route.
- **Health checks**: Periodic probing of backends, with unhealthy backends skipped automatically and recovered backends returned to the pool.
- **Longest-prefix routing**: Support for multiple routes, each with independent backends and balancing strategy.
- **Graceful shutdown**: Drains in-flight requests with a configurable timeout before exiting.
- **HTTP correctness**: Strips hop-by-hop headers, appends to `X-Forwarded-For` chains, and rewrites `Host` headers.

## Design

**Connection tracking**: The `Backend` struct wraps an atomic health flag and an atomic active connection counter. `BackendGuard` is an RAII guard that increments the counter on acquisition and decrements it on drop, so the least connections algorithm always sees an accurate count, even if a request completes unsuccessfully.

**Load balancer abstraction**: `LoadBalancer` is a trait implemented by both `RoundRobin` and `LeastConnections`. New algorithms can be added without touching the proxying or routing logic.

**Health checking**: Each route runs an independent Tokio task that polls backends on a configurable interval. Health state is written atomically, so the proxy never needs a lock to check whether a backend is available.

**Graceful shutdown**: `SIGINT` or `SIGTERM` (or on non-Unix, Ctrl+C only) sets a broadcast channel flag that the server loop checks before accepting new connections. A `tokio::time::timeout` wraps the drain phase. If in-flight requests exceed the configured timeout, the proxy logs a warning and exits anyway instead of waiting indefinitely.

## Configuration

The proxy is configured with a TOML file. By default, it looks for `reverse-proxy.toml` in the working directory, which can be overridden with the `REVERSE_PROXY_CONFIG_PATH` environment variable.

```toml
listen_addr = "127.0.0.1:3000" # Address for the reverse proxy to listen on
shutdown_timeout = 10 # Seconds to wait when draining in-flight requests during shutdown

[[routes]]
prefix = "/"
backend_addrs = ["127.0.0.1:8001", "127.0.0.1:8002", "127.0.0.1:8003"]
health_check = { path = "/health", interval_secs = 10 }
balancing_algorithm = "round_robin"

[[routes]]
prefix = "/api"
backend_addrs = ["127.0.0.1:9001"]
health_check = { path = "/ping", interval_secs = 5 }
balancing_algorithm = "least_connections"
```

Routes match on path prefix, with the longest matching prefix winning. Each route must have at least one backend on startup. Requests to routes where all backends are unhealthy receive `503 Service Unavailable`. Requests that don't match any route receive `404 Not Found`.

## Getting Started

For [Nix](https://github.com/NixOS/nix) users, the toolchain is included as a flake.

### Building and running

If not using Nix, Rust can be installed as described [here](https://rust-lang.org/tools/install/).

```sh
cargo run
```

Log level can be changed with the `RUST_LOG` environment variable (e.g. `RUST_LOG=debug cargo run`). The default level is `info`.

### Quick demo

If not using Nix, install [Python](https://www.python.org/downloads/) and [Just](https://github.com/casey/just).

```sh
just
```

This starts a Python `http.server` on port 8000, runs the proxy on port 3000, and makes a test request through it.

## Testing

```sh
cargo test
```

Tests cover route resolution, header handling, health detection and recovery, load balancing algorithms, graceful shutdown, and the shutdown timeout.

## CI

Tests, linting, format checking, and spell checking run via GitHub Actions on pushes and pull requests to `main` (as defined in [`.github/workflows/ci.yml`](.github/workflows/ci.yml)).
