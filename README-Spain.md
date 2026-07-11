# open-web-server

## Servidor web en Rust + Poem diseñado para que los artículos de pago y los datos financieros nunca se pierdan

open-web-server es un servidor web de misión crítica, 24/7/365, pensado para cargas
como compras de artículos en juegos online 3D y pagos con tarjeta de crédito.
Construido con **Rust + Poem**, trabaja junto a aruaru-db y open-runo mediante una
arquitectura de defensa en cuatro capas, de modo que los cortes de red, reinicios de
proceso y reintentos nunca causan doble cobro ni pérdida silenciosa de datos.

📖 Otros idiomas: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

## Cinco pilares

1. **Transporte de defensa en cuatro capas** (`open-web-server-wire`) — TLS 1.3 + autenticación mutua HKDF + ChaCha20-Poly1305 + protección anti-repetición seq/timestamp
2. **Escrituras a prueba de pérdidas** (`open-web-server-ledger`) — WAL previo con `Idempotency-Key` obligatoria + commit en 3 saltos
3. **Integración estrecha con aruaru-db y open-runo** — `Cliente → open-web-server → open-runo → aruaru-db`
4. **Ruta redundante UDP-IP** (`open-web-server-wire::udp_channel`, 2026-07-11) — envía en paralelo una notificación UDP cifrada y autenticada (HMAC) de mejor esfuerzo, sin reintentos (primera versión)
5. **Arquitectura objetivo: transporte y escrituras de BD con redundancia cuádruple** (revisado 2026-07-11) — objetivo final: transporte vía TCP-IP + UDP-IP + QUIC/MPQUIC + MPTCP/SCTP, escrituras hacia PostgreSQL (ACID: propiedades transaccionales de atomicidad, consistencia, aislamiento y durabilidad) + aruaru-db + replicación síncrona multi-región + registro de auditoría independiente. Por ahora solo están implementados TCP-IP y UDP-IP (sin retransmisión); el resto aún no se ha iniciado (detalles: [README-Japan.md](README-Japan.md#6-目標アーキテクチャ-通信層dbの四重化), [CLAUDE.md](CLAUDE.md)). **Próximo desarrollo planeado**: vincular los commits de aruaru-db con snapshots de ZFS (open-raid-z) — no se encontró una técnica establecida, pero se considera un hallazgo novedoso e implementable, planificado para una futura fase.

## Inicio rápido

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## Estructura (4 crates)

`open-web-server-core` (modelos de dominio/errores), `open-web-server-wire` (transporte de defensa en cuatro capas),
`open-web-server-ledger` (WAL idempotente + commit en 3 saltos), `open-web-server-gateway` (gateway Poem).

## Licencia

Apache-2.0
