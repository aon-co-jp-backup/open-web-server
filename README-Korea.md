# open-web-server

## 과금 아이템/금융 데이터가 절대 유실되지 않도록 설계된 Rust + Poem 웹 서버

open-web-server는 3D 온라인 게임 아이템 결제, 신용카드 결제 같은
미션 크리티컬한 24/7/365 워크로드를 위한 웹 서버입니다. **Rust + Poem**으로
제작되었으며, aruaru-db·open-runo와 3중 방어 아키텍처로 연동되어
재전송·프로세스 재시작·네트워크 순단이 있어도 이중 과금이나 데이터 유실이 없습니다.

📖 다른 언어: [日本語](README-Japan.md) / [English](README-English.md)

## 3대 기둥

1. **3중 방어 통신**(`open-web-server-wire`) — TLS 1.3 + HKDF 상호 인증 + ChaCha20-Poly1305
2. **유실 없는 쓰기**(`open-web-server-ledger`) — Idempotency-Key 필수 WAL 선행 기록 + 3홉 커밋
3. **aruaru-db / open-runo와의 긴밀한 통합** — `클라이언트 → open-web-server → open-runo → aruaru-db`

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
