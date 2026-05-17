# rlim — Distributed Rate Limiter

A high-performance, distributed rate limiter written in Rust. It exposes both a
binary FlatBuffers TCP interface and a standards-compliant HTTP API, and supports
five pluggable algorithms backed by either an in-memory store or etcd for
multi-node deployments.

## Features

- **Five algorithms** — Fixed Window, Sliding Window Log, Sliding Window Counter,
  Token Bucket, Leaky Bucket
- **Two transports** — FlatBuffers binary TCP (port 9000) and HTTP/JSON (port 8080)
- **Distributed state** — optional etcd backend for horizontal scaling; falls back
  to local in-memory store for single-node use
- **Standard headers** — responses conform to
  `draft-ietf-httpapi-ratelimit-headers-08` (`RateLimit-Limit`,
  `RateLimit-Remaining`, `RateLimit-Reset`, `RateLimit-Policy`, `Retry-After`)
- **HMAC signing** — optional `X-RateLimit-Mac` header (HMAC-SHA256) guards
  decisions against in-transit tampering (`strict_security` mode)
- **Kubernetes-ready** — `/health/startup`, `/health/live`, `/health/ready`
  probes; Helm chart and manifests included
- **Python client** — FastAPI demo app + `rate-limit-client` CLI in
  `clients/python/`

## Prerequisites

| Tool | Purpose |
|------|---------|
| Rust ≥ 1.77 | build server and client binaries |
| Docker + Compose | local distributed stack |
| `uv` | Python client dev (`pip install uv`) |
| `flatc` / `protoc` | code generation — auto-downloaded by `make tools` |

## Quick Start

### Single-node (in-memory, no etcd)

```bash
# 1. Build everything (downloads flatc + protoc on first run)
make all

# 2. Start the server — reads config.yaml by default
RUST_LOG=info cargo run --bin server

# 3. Check a client in another terminal
curl -s -X POST http://localhost:8080/check \
  -H 'Content-Type: application/json' \
  -d '{"client_id":"alice","resource":"/api/orders","cost":1}' | jq
```

A `200` response means the request is allowed; `429` means it was denied.

### Docker Compose (distributed with etcd)

```bash
docker compose up --build
```

This starts:
- **etcd** on `localhost:2379`
- **rate-limiter** (TCP `:9000`, HTTP `:8080`) backed by etcd
- **python-app** FastAPI demo on `localhost:8000`

To enable HMAC signing:

```bash
RATE_LIMIT_HMAC_SECRET=mysecret docker compose up --build
```

## Configuration

Copy and edit `config.yaml`:

```yaml
server:
  host: "0.0.0.0"
  port: 9000        # FlatBuffers TCP
  http_port: 8080   # HTTP API
  strict_security: false  # set true to emit X-RateLimit-Mac

# Remove this block to use the local in-memory store
# etcd:
#   endpoints:
#     - "http://etcd-0.etcd:2379"
#   key_prefix: rate-limiting

rate_limiter:
  # fixed_window | sliding_window_log | sliding_window_counter
  # token_bucket | leaky_bucket
  algorithm: token_bucket

  max_requests: 10    # fixed/sliding algorithms
  window_secs: 1

  bucket_capacity: 10 # token_bucket
  refill_rate: 2.0

  leak_rate: 2.0      # leaky_bucket
  queue_capacity: 10
```

Pass the config path as a positional argument to override the default:

```bash
cargo run --bin server -- /path/to/my-config.yaml
```

## HTTP API

### `POST /check`

```json
{ "client_id": "alice", "resource": "/api/orders", "cost": 1 }
```

`resource` and `cost` are optional (defaults: `"/"` and `1`).

| Status | Meaning |
|--------|---------|
| `200 OK` | request allowed |
| `429 Too Many Requests` | rate limit exceeded |

Response headers follow `draft-ietf-httpapi-ratelimit-headers-08`:

```
RateLimit-Limit: 10
RateLimit-Remaining: 3
RateLimit-Reset: 0
RateLimit-Policy: token_bucket;l=10
Retry-After: 1   # 429 only
```

Response body:

```json
{ "allowed": true, "remaining": 3, "retry_after_ms": 0, "reason": "ok" }
```

### Health probes

| Endpoint | k8s probe | Returns |
|----------|-----------|---------|
| `GET /health/startup` | startupProbe | `200` once server is up |
| `GET /health/live` | livenessProbe | `200` if event loop is responsive |
| `GET /health/ready` | readinessProbe | `200` / `503` based on store reachability |

## Make Targets

```
make all              Build everything (tools + codegen + Rust + Python)
make server           Build server binary only
make client           Build Rust client binary only
make test             Rust unit tests + Python unit tests
make test-unit        Python unit tests only
make test-integration Python integration tests (requires running server)
make test-e2e         Locust load test (30 s, 20 users) against localhost:8080
make clean            Remove build artifacts and Python venv
make clean-all        Also remove downloaded tools (.tools/)
make help             Show all targets
```

## Kubernetes / Helm

Manifests are in `k8s/` and a Helm chart is in `helm/rate-limiter/`.

```bash
kubectl apply -f k8s/namespace.yaml
kubectl apply -f k8s/hmac-secret.yaml   # edit before applying
kubectl apply -f k8s/configmap.yaml
kubectl apply -f k8s/etcd-statefulset.yaml
kubectl apply -f k8s/rate-limiter-statefulset.yaml
```

Or via Helm:

```bash
helm install rate-limiter helm/rate-limiter/
```

## Algorithms

| Algorithm | Best for |
|-----------|----------|
| **Fixed Window** | simple, low memory, slight burst at window boundary |
| **Sliding Window Log** | accurate, higher memory (stores full request log) |
| **Sliding Window Counter** | good accuracy with bounded memory |
| **Token Bucket** | smooth average rate, allows controlled bursts |
| **Leaky Bucket** | strict output rate, absorbs bursts via queue |

## License

MIT
