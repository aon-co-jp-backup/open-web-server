# open-web-server

## A Rust + Poem web server built so billed items and financial data are never lost

open-web-server is a mission-critical, 24/7/365 web server designed for workloads like
3D online game item purchases and credit-card financial transactions. Built with
**Rust + Poem**, it works together with aruaru-db and open-runo through a three-layer
defense architecture, so that network hiccups, process restarts, and retries never
cause double-charging or silent data loss.

📖 Other languages: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## Why open-web-server Exists

Typical billing/payment paths in web servers carry these residual risks:

| Risk | Detail |
|---|---|
| Double charging | Client retries or timeouts cause the same payment to be executed twice |
| Data loss | A write in flight is lost the instant the server process crashes |
| Weak transport | Behind a load balancer that terminates TLS, data can flow in plaintext |
| Impersonation | Service-to-service calls aren't re-verified as coming from the right peer |
| Silent failure | A DB write fails, but the client is told it succeeded anyway |

open-web-server addresses every one of these explicitly.

## Three Pillars

### 1. Three-layer defense transport (`open-web-server-wire`)

Following the same approach as aruaru-db's `aruaru-wire`, every service-to-service
call is protected by three independent layers.

| Layer | Technology | Purpose |
|---|---|---|
| Layer 1 | TLS 1.3 (rustls) | Transport encryption |
| Layer 2 | HKDF-based challenge/response | Mutual service authentication (anti-impersonation) |
| Layer 3 | ChaCha20-Poly1305 (AEAD) | Application-layer payload encryption (stays encrypted even after TLS termination) |

### 2. Loss-proof writes (`open-web-server-ledger`)

Every billing/payment request requires an `Idempotency-Key` and is committed in this order:

1. Client sends the request with an idempotency key
2. open-web-server appends it to a local write-ahead log (replayable after a process restart)
3. Forwarded to aruaru-db via open-runo (the Graph Federation Gateway)
4. aruaru-db commits it through Raft distributed consensus and issues a `commit_id`
5. The client is never told "committed" until that `commit_id` comes back

If the forward call fails partway through, it's retried automatically with exponential
backoff. Resending the same idempotency key always returns the same result, so
double-charging or double-granting cannot happen.

### 3. Tight integration with aruaru-db and open-runo

```text
Client → open-web-server → open-runo → aruaru-db
        (3-layer defense)  (3-layer defense)
```

- **open-web-server**: the client-facing entry point (REST/GraphQL, WAL pre-write)
- **open-runo**: the Federation Gateway that centralizes auth, rate limiting, and audit logs
- **aruaru-db**: the distributed Git-on-SQL database that issues an auditable hash for every commit

See [`docs/architecture.md`](docs/architecture.md) and
[`docs/integration.md`](docs/integration.md) for details.

---

## Quick Start

```bash
# 1. Start aruaru-db
cargo run -p aruaru-server -- --data ./data --raft-id 1

# 2. Start open-runo
cargo run -p open-runo-gateway

# 3. Start open-web-server
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

```bash
# Grant an item (idempotency key required)
curl -X POST http://localhost:8080/api/v1/items/grant \
  -H "Idempotency-Key: 11111111-1111-1111-1111-111111111111" \
  -H "Content-Type: application/json" \
  -d '{
    "idempotency_key": "11111111-1111-1111-1111-111111111111",
    "account_id": "user-42",
    "item_id": "sword_of_dawn",
    "quantity": 1
  }'
```

## Project Layout

```text
open-web-server/
├── crates/
│   ├── open-web-server-core/     # domain models and error types
│   ├── open-web-server-wire/     # 3-layer defense transport (TLS / mutual auth / payload encryption)
│   ├── open-web-server-ledger/   # idempotent WAL + 3-hop commit pipeline
│   └── open-web-server-gateway/  # Poem-based web gateway (binary)
├── docs/
│   ├── architecture.md
│   └── integration.md
└── Cargo.toml (workspace)
```

## Roadmap

- [ ] Extract `MutationRequest`/`MutationReceipt` into a shared `open-cosmo` crate
- [ ] Add GraphQL endpoints (`poem-openapi` / `async-graphql`)
- [ ] Tauri-based admin console matching the open-runo/aruaru-db admin UI
- [ ] End-to-end tracing via OpenTelemetry

## License

Apache-2.0
