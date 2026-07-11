# open-web-server

## 基于 Rust + Poem 构建、确保计费物品和金融数据永不丢失的 Web 服务器

open-web-server 是面向 3D 网游道具购买、信用卡支付等关键业务的 24/7/365 Web 服务器。
使用 **Rust + Poem** 构建，通过四层防御架构与 aruaru-db、open-runo 协同工作，
确保网络抖动、进程重启、重试都不会导致重复扣款或数据静默丢失。

📖 其他语言: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

## 五大支柱

1. **四层防御通信**(`open-web-server-wire`)— TLS 1.3 + HKDF 双向认证 + ChaCha20-Poly1305 + seq/timestamp 防重放
2. **不丢失的写入**(`open-web-server-ledger`)— 强制 Idempotency-Key 的 WAL 预写 + 三跳提交
3. **与 aruaru-db / open-runo 紧密集成** — `客户端 → open-web-server → open-runo → aruaru-db`
4. **UDP-IP 冗余通道**(`open-web-server-wire::udp_channel`,2026-07-11)— 与 TCP 权威提交并行,以加密+HMAC 方式尽力发送 UDP 即时通知(无重传,第一版实现)
5. **目标架构:通信层与数据库写入的四重冗余**(2026-07-11修订)— 最终目标:通信层采用 TCP-IP + UDP-IP + QUIC/MPQUIC + MPTCP/SCTP 四种方式,数据库写入采用 PostgreSQL(ACID,即原子性、一致性、隔离性、持久性的事务保证特性) + aruaru-db + 多区域同步复制 + 独立审计日志四条路径。目前仅实现了 TCP-IP 与 UDP-IP(无重传),其余尚未开始(详见 [README-Japan.md](README-Japan.md#6-目標アーキテクチャ-通信層dbの四重化) 与 [CLAUDE.md](CLAUDE.md))。**下一步新开发计划**:将 aruaru-db 的提交与 ZFS(open-raid-z)快照联动——虽未找到现成技术,但判断为可实现的新发现,计划下一阶段着手。

## 快速开始

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## 项目结构(4 个 crate)

`open-web-server-core`(领域模型/错误类型)、`open-web-server-wire`(四层防御通信)、
`open-web-server-ledger`(幂等 WAL + 三跳提交)、`open-web-server-gateway`(Poem 网关)。

## License

Apache-2.0
