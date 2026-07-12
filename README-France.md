# open-web-server

## Serveur web en Rust + Poem conçu pour que les articles payants et les données financières ne soient jamais perdus

open-web-server est un serveur web critique, fonctionnant 24/7/365, conçu pour des
charges comme les achats d'objets dans des jeux en ligne 3D ou les paiements par
carte bancaire. Construit avec **Rust + Poem**, il fonctionne avec aruaru-db et
open-runo via une architecture de défense à quatre couches, garantissant qu'aucune
coupure réseau, redémarrage de processus ou nouvelle tentative ne provoque de
double facturation ni de perte silencieuse de données.

📖 Autres langues : [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

## Cinq piliers

1. **Transport de défense à quatre couches** (`open-web-server-wire`) — TLS 1.3 + authentification mutuelle HKDF + ChaCha20-Poly1305 + protection anti-rejeu seq/timestamp
2. **Écritures à l'épreuve des pertes** (`open-web-server-ledger`) — WAL préalable avec `Idempotency-Key` obligatoire + commit en 3 sauts
3. **Intégration étroite avec aruaru-db et open-runo** — `Client → open-web-server → open-runo → aruaru-db`
4. **Voie redondante UDP-IP** (`open-web-server-wire::udp_channel`, 2026-07-11) — envoie en parallèle une notification UDP chiffrée + authentifiée (HMAC) au mieux, sans nouvelle tentative (première version)
5. **Architecture cible : transport et écritures DB quadruple-redondants** (révisé 2026-07-11) — objectif à terme : transport via TCP-IP + UDP-IP + QUIC/MPQUIC + MPTCP/SCTP, écritures vers PostgreSQL (propriétés transactionnelles ACID : atomicité, cohérence, isolation, durabilité) + aruaru-db + réplication synchrone multi-région + journal d'audit indépendant. Désormais TCP-IP, UDP-IP, ainsi que ③QUIC (`quinn`, vérifié par une poignée de main TLS1.3 réelle) et un WAL PostgreSQL (`sqlx`, avec BEGIN/COMMIT réel ; non vérifié contre un PostgreSQL réel, indisponible dans ce bac à sable) sont implémentés ; ④MPTCP/SCTP : implementation/verification au niveau noyau jugee infaisable dans ce bac a sable Windows ; un substitut en espace utilisateur (`aggligator`, `open-web-server-wire::mptcp_channel`, explicitement pas du vrai MPTCP/SCTP noyau) a ete verifie via un round-trip TCP reel en loopback (2026-07-13). **④ le journal d'audit independant est desormais implemente** (`open-web-server-ledger::audit_log::FileAuditLog`, 2026-07-13, fichier append-only avec checksum SHA-256, techniquement independant du WAL/aruaru-db, avec `scan_and_verify()`/`reconcile()`). Le reste (②aruaru-db, ③replication multi-region) reste a faire (détails : [README-Japan.md](README-Japan.md#6-目標アーキテクチャ-通信層dbの四重化), [CLAUDE.md](CLAUDE.md)). **Prochain développement prévu** : associer les commits d'aruaru-db aux snapshots ZFS (open-raid-z) — aucune technique établie trouvée, mais considéré comme une découverte nouvelle et réalisable, prévue pour une future passe.

## Démarrage rapide

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## Structure (4 crates)

`open-web-server-core` (modèles de domaine/erreurs), `open-web-server-wire` (transport de défense à quatre couches),
`open-web-server-ledger` (WAL idempotent + commit en 3 sauts), `open-web-server-gateway` (gateway Poem).

## Licence

Apache-2.0
