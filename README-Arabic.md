# open-web-server

## خادم ويب مبني بلغة Rust وإطار Poem، مصمم كي لا تُفقد العناصر المدفوعة والبيانات المالية أبدًا

open-web-server هو خادم ويب حيوي يعمل على مدار الساعة طوال الأسبوع، مخصص
لأحمال عمل مثل شراء العناصر في ألعاب أونلاين ثلاثية الأبعاد والمدفوعات
ببطاقات الائتمان. مبني بلغة **Rust وإطار Poem**، ويعمل جنبًا إلى جنب مع
aruaru-db و open-runo عبر معمارية دفاع ثلاثية الطبقات، بحيث لا تؤدي انقطاعات
الشبكة أو إعادة تشغيل العمليات أو إعادة المحاولات أبدًا إلى خصم مزدوج أو
فقدان صامت للبيانات.

📖 لغات أخرى: [日本語](README-Japan.md) / [English](README-English.md)

## الركائز الثلاث

1. **نقل دفاعي ثلاثي الطبقات** (`open-web-server-wire`) — TLS 1.3 + مصادقة متبادلة عبر HKDF + تشفير ChaCha20-Poly1305
2. **كتابة لا تُفقد** (`open-web-server-ledger`) — سجل WAL مسبق مع `Idempotency-Key` إلزامي + التزام عبر 3 قفزات
3. **تكامل وثيق مع aruaru-db و open-runo** — `العميل → open-web-server → open-runo → aruaru-db`

## البدء السريع

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## البنية (4 حزم)

`open-web-server-core` (نماذج المجال/الأخطاء)، `open-web-server-wire` (النقل الدفاعي ثلاثي الطبقات)،
`open-web-server-ledger` (WAL متكافئ + التزام عبر 3 قفزات)، `open-web-server-gateway` (بوابة Poem).

## الترخيص

Apache-2.0
