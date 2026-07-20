# open-web-server

## Ein Rust + Poem Webserver, der dafür sorgt, dass Bezahl-Items und Finanzdaten niemals verloren gehen

open-web-server ist ein geschäftskritischer 24/7/365-Webserver für Workloads wie
Item-Käufe in 3D-Online-Spielen und Kreditkartenzahlungen. Gebaut mit **Rust + Poem**,
arbeitet er über eine vierschichtige Verteidigungsarchitektur mit aruaru-db und
open-runo zusammen, sodass Netzwerkaussetzer, Prozessneustarts und Wiederholungen
niemals zu Doppelbuchungen oder stillem Datenverlust führen.

📖 Weitere Sprachen: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

## Fünf Säulen

1. **Vierschichtige Verteidigungsübertragung** (`open-web-server-wire`) — TLS 1.3 + HKDF-basierte gegenseitige Authentifizierung + ChaCha20-Poly1305 + seq/timestamp-Replay-Schutz
2. **Verlustsichere Schreibvorgänge** (`open-web-server-ledger`) — Vorab-WAL mit Pflicht-`Idempotency-Key` + Commit über 3 Hops
3. **Enge Integration mit aruaru-db und open-runo** — `Client → open-web-server → open-runo → aruaru-db`
4. **Redundanter UDP-IP-Pfad** (`open-web-server-wire::udp_channel`, 2026-07-11) — sendet parallel zum TCP-Commit eine verschlüsselte, HMAC-authentifizierte UDP-Benachrichtigung nach Best-Effort-Prinzip (kein Retransmit, erste Version)
5. **Zielarchitektur: vierfach redundanter Transport und vierfach redundante DB-Schreibvorgänge** (überarbeitet 2026-07-11) — Zielbild: Transport über TCP-IP + UDP-IP + QUIC/MPQUIC + MPTCP/SCTP, DB-Schreibvorgänge über PostgreSQL (ACID-Transaktionseigenschaften: Atomarität, Konsistenz, Isolation, Dauerhaftigkeit) + aruaru-db + synchrone Multi-Region-Replikation + unabhängiges Audit-Log. Aktuell sind TCP-IP, UDP-IP sowie ③QUIC (`quinn`, mit echtem TLS1.3-Handshake verifiziert) und ein PostgreSQL-WAL (`sqlx`, mit echtem BEGIN/COMMIT; mangels erreichbarem PostgreSQL in dieser Sandbox unverifiziert) implementiert; ④MPTCP/SCTP: Kernel-Implementierung/Verifikation in dieser Windows-Sandbox als nicht machbar bestaetigt; stattdessen ein Userspace-Ersatz (`aggligator`, `open-web-server-wire::mptcp_channel`, ausdruecklich kein echtes Kernel-MPTCP/SCTP) implementiert und per echtem Loopback-TCP-Roundtrip verifiziert (2026-07-13). **④ das unabhaengige Audit-Log ist jetzt implementiert** (`open-web-server-ledger::audit_log::FileAuditLog`, 2026-07-13, Append-only-Datei mit SHA-256-Checksumme, technisch unabhaengig von WAL/aruaru-db, mit `scan_and_verify()`/`reconcile()`). Der Rest (②aruaru-db, ③Multi-Region-Replikation) ist noch offen (Details: [README-Japan.md](README-Japan.md#6-目標アーキテクチャ-通信層dbの四重化), [CLAUDE.md](CLAUDE.md)). **Geplante nächste Neuentwicklung**: aruaru-db-Commits mit ZFS-Snapshots (open-raid-z) koppeln — keine etablierte Technik gefunden, aber als neuartige, umsetzbare Erkenntnis eingestuft und für einen künftigen Durchlauf geplant.

6. **Statisches Datei- + PHP-Serving** (`static_files`/`php_server`/`web_vhost`, 2026-07-20) — erster Schritt zu einer hybriden Apache+Nginx-Auslieferungs-Engine. Jeder Hostname wird auf ein Docroot abgebildet; Pfade, die anhand der Dateiendung als statische Assets erkennbar sind, werden direkt von der Festplatte ausgeliefert (mit Path-Traversal-Schutz), der Rest wird an einen bei Bedarf gestarteten `php -S`-Subprozess weitergeleitet. Verifiziert durch tatsächliches Ausliefern von `audiocafe.tokyo` (einer bestehenden PHP-Site).

## Schnellstart

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## Struktur (4 Crates)

`open-web-server-core` (Domänenmodelle/Fehlertypen), `open-web-server-wire` (vierschichtige Verteidigungsübertragung),
`open-web-server-ledger` (idempotentes WAL + Commit über 3 Hops), `open-web-server-gateway` (Poem-Gateway).

## Lizenz

Apache-2.0
