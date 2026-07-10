# open-web-server

## Веб-сервер на Rust + Poem, спроектированный так, чтобы платные предметы и финансовые данные никогда не терялись

open-web-server — критически важный веб-сервер, работающий 24/7/365, для таких
нагрузок, как покупка внутриигровых предметов в 3D-онлайн играх и платежи по
кредитным картам. Построен на **Rust + Poem** и работает вместе с aruaru-db и
open-runo через трёхуровневую защитную архитектуру, благодаря чему сетевые сбои,
перезапуски процессов и повторные попытки никогда не приводят к двойному списанию
или незаметной потере данных.

📖 Другие языки: [日本語](README-Japan.md) / [English](README-English.md)

## Три опоры

1. **Трёхуровневая защищённая передача** (`open-web-server-wire`) — TLS 1.3 + взаимная аутентификация на основе HKDF + ChaCha20-Poly1305
2. **Записи без потерь** (`open-web-server-ledger`) — предварительный WAL с обязательным `Idempotency-Key` + коммит в 3 шага
3. **Тесная интеграция с aruaru-db и open-runo** — `Клиент → open-web-server → open-runo → aruaru-db`

## Быстрый старт

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## Структура (4 крейта)

`open-web-server-core` (доменные модели/ошибки), `open-web-server-wire` (трёхуровневая защищённая передача),
`open-web-server-ledger` (идемпотентный WAL + коммит в 3 шага), `open-web-server-gateway` (шлюз на Poem).

## Лицензия

Apache-2.0
