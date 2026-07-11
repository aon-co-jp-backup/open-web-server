# open-web-server

## 과금 아이템/금융 데이터가 절대 유실되지 않도록 설계된 Rust + Poem 웹 서버

open-web-server는 3D 온라인 게임 아이템 결제, 신용카드 결제 같은
미션 크리티컬한 24/7/365 워크로드를 위한 웹 서버입니다. **Rust + Poem**으로
제작되었으며, aruaru-db·open-runo와 3중 방어 아키텍처로 연동되어
재전송·프로세스 재시작·네트워크 순단이 있어도 이중 과금이나 데이터 유실이 없습니다.

📖 다른 언어: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

## 5대 기둥

1. **3중 방어 통신**(`open-web-server-wire`) — TLS 1.3 + HKDF 상호 인증 + ChaCha20-Poly1305
2. **유실 없는 쓰기**(`open-web-server-ledger`) — Idempotency-Key 필수 WAL 선행 기록 + 3홉 커밋
3. **aruaru-db / open-runo와의 긴밀한 통합** — `클라이언트 → open-web-server → open-runo → aruaru-db`
4. **UDP-IP 이중 경로**(`open-web-server-wire::udp_channel`, 2026-07-11) — TCP 권위 커밋과 병행하여 암호화+HMAC이 적용된 UDP 즉시 알림을 최선형으로 전송(재전송 없음, 첫 구현)
5. **목표 아키텍처: 통신·DB 쓰기의 4중 이중화**(2026-07-11 개정) — 최종 목표는 통신층에서 TCP-IP + UDP-IP + QUIC/MPQUIC + MPTCP/SCTP 4가지 방식을, DB 쓰기에서 PostgreSQL(ACID — 원자성·일관성·고립성·지속성을 보장하는 트랜잭션 특성) + aruaru-db + 다중 리전 동기 복제 + 독립 감사 로그 4계통을 병행하는 것입니다. 현재는 TCP-IP와 UDP-IP(재전송 없음)만 구현되어 있으며 나머지는 아직 착수 전입니다 (자세한 내용: [README-Japan.md](README-Japan.md#6-目標アーキテクチャ-通信層dbの四重化), [CLAUDE.md](CLAUDE.md)). **다음 신규 개발 예정**: aruaru-db 커밋과 ZFS(open-raid-z) 스냅샷 연동 — 확립된 기법은 찾지 못했지만 실현 가능한 새로운 발견으로 판단하여 다음 패스에서 착수 예정입니다.

## 빠른 시작

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## 구성(4개 crate)

`open-web-server-core`(도메인 모델/에러 타입), `open-web-server-wire`(3중 방어 통신),
`open-web-server-ledger`(멱등 WAL + 3홉 커밋), `open-web-server-gateway`(Poem 게이트웨이).

## License

Apache-2.0
