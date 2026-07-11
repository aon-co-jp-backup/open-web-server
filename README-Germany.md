# open-web-server

## Ein Rust + Poem Webserver, der dafür sorgt, dass Bezahl-Items und Finanzdaten niemals verloren gehen

open-web-server ist ein geschäftskritischer 24/7/365-Webserver für Workloads wie
Item-Käufe in 3D-Online-Spielen und Kreditkartenzahlungen. Gebaut mit **Rust + Poem**,
arbeitet er über eine dreischichtige Verteidigungsarchitektur mit aruaru-db und
open-runo zusammen, sodass Netzwerkaussetzer, Prozessneustarts und Wiederholungen
niemals zu Doppelbuchungen oder stillem Datenverlust führen.

📖 Weitere Sprachen: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

## Vier Säulen

1. **Dreischichtige Verteidigungsübertragung** (`open-web-server-wire`) — TLS 1.3 + HKDF-basierte gegenseitige Authentifizierung + ChaCha20-Poly1305
2. **Verlustsichere Schreibvorgänge** (`open-web-server-ledger`) — Vorab-WAL mit Pflicht-`Idempotency-Key` + Commit über 3 Hops
3. **Enge Integration mit aruaru-db und open-runo** — `Client → open-web-server → open-runo → aruaru-db`
4. **Redundanter UDP-IP-Pfad** (`open-web-server-wire::udp_channel`, 2026-07-11) — sendet parallel zum TCP-Commit eine verschlüsselte, HMAC-authentifizierte UDP-Benachrichtigung nach Best-Effort-Prinzip (kein Retransmit, erste Version)

## Schnellstart

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## Struktur (4 Crates)

`open-web-server-core` (Domänenmodelle/Fehlertypen), `open-web-server-wire` (dreischichtige Verteidigungsübertragung),
`open-web-server-ledger` (idempotentes WAL + Commit über 3 Hops), `open-web-server-gateway` (Poem-Gateway).

## Lizenz

Apache-2.0
