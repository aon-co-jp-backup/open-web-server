# open-web-server

## Un server web in Rust + Poem progettato affinché oggetti a pagamento e dati finanziari non vadano mai persi

open-web-server è un server web mission-critical, 24/7/365, pensato per carichi
come gli acquisti di oggetti in giochi online 3D e i pagamenti con carta di credito.
Costruito con **Rust + Poem**, collabora con aruaru-db e open-runo tramite
un'architettura a difesa su tre livelli, così che interruzioni di rete, riavvii di
processo e nuovi tentativi non causino mai doppi addebiti o perdite silenziose di dati.

📖 Altre lingue: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

## Quattro pilastri

1. **Trasporto a difesa su tre livelli** (`open-web-server-wire`) — TLS 1.3 + autenticazione reciproca HKDF + ChaCha20-Poly1305
2. **Scritture a prova di perdita** (`open-web-server-ledger`) — WAL preventivo con `Idempotency-Key` obbligatoria + commit in 3 hop
3. **Integrazione stretta con aruaru-db e open-runo** — `Client → open-web-server → open-runo → aruaru-db`
4. **Percorso ridondante UDP-IP** (`open-web-server-wire::udp_channel`, 2026-07-11) — invia in parallelo al commit TCP autoritativo una notifica UDP cifrata e autenticata (HMAC) con best-effort, senza ritrasmissione (prima versione)

## Avvio rapido

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## Struttura (4 crate)

`open-web-server-core` (modelli di dominio/errori), `open-web-server-wire` (trasporto a difesa su tre livelli),
`open-web-server-ledger` (WAL idempotente + commit in 3 hop), `open-web-server-gateway` (gateway Poem).

## Licenza

Apache-2.0
