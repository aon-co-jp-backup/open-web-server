# open-web-server

## 基于 Rust + Poem 构建、确保计费物品和金融数据永不丢失的 Web 服务器

open-web-server 是面向 3D 网游道具购买、信用卡支付等关键业务的 24/7/365 Web 服务器。
使用 **Rust + Poem** 构建，通过三层防御架构与 aruaru-db、open-runo 协同工作，
确保网络抖动、进程重启、重试都不会导致重复扣款或数据静默丢失。

📖 其他语言: [日本語](README-Japan.md) / [English](README-English.md)

## 三大支柱

1. **三层防御通信**(`open-web-server-wire`)— TLS 1.3 + HKDF 双向认证 + ChaCha20-Poly1305
2. **不丢失的写入**(`open-web-server-ledger`)— 强制 Idempotency-Key 的 WAL 预写 + 三跳提交
3. **与 aruaru-db / open-runo 紧密集成** — `客户端 → open-web-server → open-runo → aruaru-db`

## 快速开始

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## 项目结构(4 个 crate)

`open-web-server-core`(领域模型/错误类型)、`open-web-server-wire`(三层防御通信)、
`open-web-server-ledger`(幂等 WAL + 三跳提交)、`open-web-server-gateway`(Poem 网关)。

## License

Apache-2.0
