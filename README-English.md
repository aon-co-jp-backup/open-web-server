# open-web-server

## A Rust + tokio/hyper web server built so billed items and financial data are never lost

open-web-server is a mission-critical, 24/7/365 web server designed for workloads like
3D online game item purchases and credit-card financial transactions. Built directly on
**Rust + tokio/hyper** (no web framework dependency), it works together with aruaru-db and open-runo through a four-layer
defense architecture, so that network hiccups, process restarts, and retries never
cause double-charging or silent data loss.

> Note: the routing/handler API shape is kept compatible with the earlier Poem-based
> implementation, but the package itself no longer depends on Poem (migrated to a direct
> tokio/hyper implementation on 2026-07-10).

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

### 1. Four-layer defense transport (`open-web-server-wire`)

Following the same approach as aruaru-db's `aruaru-wire`, every service-to-service
call is protected by four independent layers.

| Layer | Technology | Purpose |
|---|---|---|
| Layer 1 | TLS 1.3 (rustls) | Transport encryption |
| Layer 2 | HKDF-based challenge/response | Mutual service authentication (anti-impersonation) |
| Layer 3 | ChaCha20-Poly1305 (AEAD) | Application-layer payload encryption (stays encrypted even after TLS termination) |
| Layer 4 | seq/timestamp replay guard (`replay_guard`, added 2026-07-11) | Rejects replay of already-valid ciphertext (prevents double-charging/double-granting) |

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
        (4-layer defense)  (4-layer defense)
```

- **open-web-server**: the client-facing entry point (REST/GraphQL, WAL pre-write)
- **open-runo**: the Federation Gateway that centralizes auth, rate limiting, and audit logs
- **aruaru-db**: the distributed Git-on-SQL database that issues an auditable hash for every commit

See [`docs/architecture.md`](docs/architecture.md) and
[`docs/integration.md`](docs/integration.md) for details.

### 4. OpenTelemetry tracing (`open-web-server-gateway::telemetry`)

The `grant_item`/`charge` handlers are instrumented with `tracing::instrument`
spans, which are exported as OpenTelemetry traces via `tracing-opentelemetry`.

- If `OTEL_EXPORTER_OTLP_ENDPOINT` is set, spans are shipped to a Collector
  over OTLP/HTTP (protobuf) — intended for production/staging.
- If unset, spans are written to stdout instead (a local-dev fallback for
  when no Collector is running).

This is the groundwork for tracing the full
`Client → open-web-server → open-runo → aruaru-db` call chain as a single
distributed trace once `open-runo`/`aruaru-db` wire up compatible exporters
(their current status here is unverified).

### 5. UDP-IP redundant transport path (`open-web-server-wire::udp_channel`, 2026-07-11)

To make billing/financial transactions harder to lose in flight, mutations
are now also sent over a best-effort UDP side channel **in parallel** with
the existing TCP-authoritative path. This does not replace the four-layer
defense transport (TLS / mutual auth / payload encryption / replay guard) — it's an
orthogonal capability: redundancy of the transport path itself.

- `open-web-server-ledger::Ledger::commit()` fires the UDP send via
  `tokio::spawn` right after the WAL pre-write, as pure fire-and-forget.
  The TCP-authoritative commit (`forward_with_retry`) is never blocked by
  it, and the commit still succeeds even if the UDP send fails or the
  destination is unreachable.
- Datagrams are encrypted with `PayloadCipher` (ChaCha20-Poly1305 AEAD) and
  authenticated with an HMAC-SHA256 tag per datagram, since UDP has no TLS
  of its own.
- The receiving side deduplicates by `IdempotencyKey` (`Deduplicator`), so
  a mutation arriving over both TCP and UDP is never double-applied.

**Honest scope limits**: there is no UDP retransmit — it is a pure
send-and-hope "advance notice." Of the target "primary TCP + secondary TCP
+ UDP" triple-redundancy, this is a first cut covering only the single UDP
path; a secondary TCP path is not implemented yet. Wiring an actual
receiving-side listener into open-runo is also out of scope for this
repository right now — this crate provides the sending side plus a
verifiable receiving implementation for tests. See
[`docs/architecture.md`](docs/architecture.md#冗長化された伝送経路-tcp-ip--udp-ip-open-web-server-wireudp_channel-2026-07-11)
for details.

### 6. Target architecture: quadruple-redundant transport and DB writes

(Revised 2026-07-11: the original "triple-redundant TCP+UDP" concept was
expanded to "quadruple-redundant" after research showed that a TCP+UDP-only
approach isn't sufficient by current standards. This is a target
architecture to be implemented incrementally, per user instruction — the
following is the full end-state picture. See
[`CLAUDE.md`](CLAUDE.md#拡張要件2026-07-11ユーザー指示目標アーキテクチャ実装は段階的に)
for details and cited sources.)

To keep billed items, financial data, securities data, and credit-card data
in 3D online games from being lost over the network, `open-web-server`
combines with `poem-cosmo-tauri` (or `open-runo`), `PostgreSQL`,
`aruaru-db`, and `open-raid-z` toward the following target:

- **Quadruple-redundant transport**: four transport methods with different
  failure characteristics running in parallel — ① TCP-IP, ② UDP-IP,
  ③ QUIC (ideally Multipath QUIC / MPQUIC), and ④ Multipath TCP (MPTCP) or
  SCTP.
- **Quadruple-redundant database writes**: the same transaction reflected
  to four independent persistence targets — ① PostgreSQL (ACID —
  atomicity, consistency, isolation, and durability transaction
  guarantees), ② aruaru-db, ③ multi-region synchronous replication,
  and ④ an independent audit/reconciliation transaction log.

**Honest status as of 2026-07-12**: on the transport side, ① TCP-IP and
② UDP-IP remain implemented, and **③ QUIC has now been implemented**
(`open-web-server-wire::quic_channel`, built on the `quinn` crate, with
built-in TLS 1.3 and a dev/test self-signed certificate helper; verified
with a real TLS handshake and bidirectional stream round-trip over a real
UDP socket in an integration test — this is single-path QUIC, not
Multipath QUIC/MPQUIC, which remains out of scope). **④ MPTCP/SCTP was
investigated and found genuinely infeasible to implement/verify on this
Windows sandbox** (honest blocker: no native kernel MPTCP on Windows, no
viable Windows SCTP stack). A user-space substitute serving the same
purpose (physical-path multihoming) was implemented instead using the
`aggligator` crate (`open-web-server-wire::mptcp_channel`, explicitly
documented as not real kernel MPTCP/SCTP), verified with a real loopback
TCP round-trip integration test (2026-07-13). On the DB side, **① PostgreSQL now has a first-cut write
path** (`open-web-server-ledger::PostgresWal`, built on `sqlx`, with a
real `BEGIN`/`COMMIT` transaction boundary and an idempotent
`INSERT ... ON CONFLICT DO NOTHING` for the write-ahead append — **but
this sandbox has no reachable live PostgreSQL instance, so the live-DB
path itself is unverified**; only the SQL-construction logic is unit
tested, plus an `#[ignore]`d integration test that runs only when
`DATABASE_URL` is set). **④ an independent audit/reconciliation log is
now implemented** (`open-web-server-ledger::audit_log::FileAuditLog`,
2026-07-13): an append-only file, technically independent from
PostgreSQL/aruaru-db/multi-region replication, that records one
SHA-256-checksummed record per commit attempt. `scan_and_verify()`
detects silent corruption; `reconcile()` cross-checks the audit log's
key set against the WAL's committed-key set to flag entries missing
from the WAL or duplicated within the audit log. Enabled via
`Ledger::enable_audit_log(path)`; the append happens right after the
WAL write and never blocks the authoritative path on failure. Verified
with 4 tests covering round-trip file I/O, corruption detection,
reconciliation reporting, and end-to-end wiring through
`Ledger::commit`. **③ multi-region synchronous replication is now also
implemented** (`open-web-server-ledger::multi_region::
MultiRegionReplicator`, 2026-07-13): two or more real SQLite files stand
in for "regions," and a commit writes **synchronously to every region,
waiting for all acknowledgements before returning success** — the
opposite of the UDP path's fire-and-forget model. The failure policy is
explicit and chosen: the default is strict (any single region failure
fails the whole commit), with an opt-in `with_quorum(n)` N-of-M
degraded mode. Verified with 4 real-file-I/O tests covering the happy
path, quorum degradation, and total-failure cases. **This means all
four quadruple-redundant DB-write legs (①②③④) are now implemented**
(① is the only one not verified against a live instance). **② the
aruaru-db × ZFS-snapshot pairing is also now implemented**, on the
`aruaru-db` side (`aruaru-dist::snapshot_pairing`, a real Raft commit
triggering a real `open-raid-z` snapshot, verified against a real
RAID-Z2 pool — see `aruaru-db`'s own `CLAUDE.md` for detail). The
VersionLessAPI + Git-versioning hybrid already works on the write side
(the `commit_id` is surfaced back to the client via `MutationReceipt`),
but the read side — querying past state by `commit_id` — doesn't exist
yet and needs coordination with `open-runo`'s API surface; that's the
next concrete gap. Integration with `open-raid-z` as open-web-server's
own disk-redundancy foundation is likewise not yet started.

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

Send that same request again with the **same** `Idempotency-Key` and the item will
not be granted twice — this is the §0 zero-loss mission working as intended. Once
`db_commit_id` in the response is non-null, the write is durably committed in `aruaru-db`.

Charging a card works the same way:

```bash
curl -X POST http://localhost:8080/api/v1/transactions/charge \
  -H "Idempotency-Key: 22222222-2222-2222-2222-222222222222" \
  -H "Content-Type: application/json" \
  -d '{
    "idempotency_key": "22222222-2222-2222-2222-222222222222",
    "account_id": "user-42",
    "amount_cents": 999,
    "currency": "USD"
  }'
```

Health check (no auth required): `curl http://localhost:8080/healthz`

Environment variables: `OPEN_RUNO_ENDPOINT` (default `https://127.0.0.1:8443`),
`OPEN_WEB_SERVER_BIND` (default `0.0.0.0:8080`).

## Adding a New Endpoint (Minimal Example)

Since there's no web framework, adding an endpoint is three steps: define a
request/response type, write a handler function, add one line to `dispatch()`
in `main.rs`. Here's a minimal example adding `GET /api/v1/items/status`:

```rust
// crates/open-web-server-gateway/src/handlers/items.rs

/// `GET /api/v1/items/status?account_id=user-42`
pub async fn item_status(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    let account_id = req
        .uri()
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("account_id=")))
        .unwrap_or_default()
        .to_string();

    // Replace with a real lookup via state.ledger or your own query path
    json_response(StatusCode::OK, &serde_json::json!({ "account_id": account_id }))
}
```

```rust
// crates/open-web-server-gateway/src/main.rs, inside dispatch()

match (method, path.as_str()) {
    (Method::POST, "/api/v1/items/grant") => handlers::items::grant_item(state, req).await,
    (Method::GET, "/api/v1/items/status") => handlers::items::item_status(state, req).await, // new
    (Method::POST, "/api/v1/transactions/charge") => {
        handlers::transactions::charge(state, req).await
    }
    (Method::GET, "/healthz") => text_response(StatusCode::OK, "ok"),
    _ => text_response(StatusCode::NOT_FOUND, "not found"),
}
```

If your new endpoint is a mutating (POST/PUT) call that touches money or items,
add its path prefix to the `needs_key` check in
`crates/open-web-server-gateway/src/middleware/idempotency.rs` — otherwise it
won't get the mandatory `Idempotency-Key` enforcement that the zero-loss
mission (§0 in `docs/HYBRID_NETWORK_ARCHITECTURE.md`) depends on.

## Project Layout

```text
open-web-server/
├── crates/
│   ├── open-web-server-core/     # domain models and error types
│   ├── open-web-server-wire/     # 4-layer defense transport (TLS / mutual auth / payload encryption / replay guard)
│   ├── open-web-server-ledger/   # idempotent WAL + 3-hop commit pipeline
│   └── open-web-server-gateway/  # tokio/hyper web gateway (binary; Poem-API-compatible, no Poem dependency)
├── docs/
│   ├── architecture.md
│   └── integration.md
└── Cargo.toml (workspace)
```

## Roadmap

- [ ] Extract `MutationRequest`/`MutationReceipt` into a shared `open-cosmo` crate
  (check `open-runo`/`aruaru-db` progress on this before starting)
- [ ] Add GraphQL endpoints (e.g. `async-graphql`)
- [ ] Rust-to-WASM admin console matching the open-runo/aruaru-db admin UI
  (the 2026-07-10 stack pivot means this is Rust/WASM, not Tauri)
- [x] Tracing via OpenTelemetry (implemented on the `open-web-server-gateway`
  side; true end-to-end tracing still depends on `open-runo`/`aruaru-db`
  adopting compatible exporters)
- [x] First cut of the UDP-IP redundant transport path
  (`open-web-server-wire::udp_channel`; retransmit, a secondary TCP path,
  and an open-runo-side receiving listener remain future work)

## License

Apache-2.0
