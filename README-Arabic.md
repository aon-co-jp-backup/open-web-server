# open-web-server

## خادم ويب مبني بلغة Rust وإطار Poem، مصمم كي لا تُفقد العناصر المدفوعة والبيانات المالية أبدًا

open-web-server هو خادم ويب حيوي يعمل على مدار الساعة طوال الأسبوع، مخصص
لأحمال عمل مثل شراء العناصر في ألعاب أونلاين ثلاثية الأبعاد والمدفوعات
ببطاقات الائتمان. مبني بلغة **Rust وإطار Poem**، ويعمل جنبًا إلى جنب مع
aruaru-db و open-runo عبر معمارية دفاع رباعية الطبقات، بحيث لا تؤدي انقطاعات
الشبكة أو إعادة تشغيل العمليات أو إعادة المحاولات أبدًا إلى خصم مزدوج أو
فقدان صامت للبيانات.

📖 لغات أخرى: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

## الركائز الخمس

1. **نقل دفاعي رباعي الطبقات** (`open-web-server-wire`) — TLS 1.3 + مصادقة متبادلة عبر HKDF + تشفير ChaCha20-Poly1305 + حماية من إعادة التشغيل (seq/timestamp)
2. **كتابة لا تُفقد** (`open-web-server-ledger`) — سجل WAL مسبق مع `Idempotency-Key` إلزامي + التزام عبر 3 قفزات
3. **تكامل وثيق مع aruaru-db و open-runo** — `العميل → open-web-server → open-runo → aruaru-db`
4. **مسار UDP-IP احتياطي** (`open-web-server-wire::udp_channel`، 2026-07-11) — يرسل بالتوازي مع التزام TCP الرسمي إشعار UDP مشفّرًا وموثّقًا (HMAC) على أساس أفضل جهد، دون إعادة إرسال (نسخة أولى)
5. **البنية المستهدفة: تكرار رباعي للنقل وكتابة قاعدة البيانات** (مُنقّح 2026-07-11) — الهدف النهائي: النقل عبر TCP-IP + UDP-IP + QUIC/MPQUIC + MPTCP/SCTP، والكتابة إلى PostgreSQL (خصائص معاملات ACID: الذرية، الاتساق، العزل، الديمومة) + aruaru-db + النسخ المتزامن متعدد المناطق + سجل تدقيق مستقل. حاليًا تم تنفيذ TCP-IP وUDP-IP، بالإضافة إلى ③QUIC (`quinn`، تم التحقق منه عبر مصافحة TLS1.3 حقيقية) وWAL لقاعدة PostgreSQL (`sqlx`، بمعاملات BEGIN/COMMIT حقيقية؛ لم يُتحقق منه مقابل PostgreSQL حقيقي لعدم توفره في هذه البيئة المعزولة)؛ أما ④MPTCP/SCTP: تبيّن أن التنفيذ/التحقق على مستوى النواة غير ممكن في بيئة Windows المعزولة هذه، فتم تنفيذ بديل في مساحة المستخدم (`aggligator`، `open-web-server-wire::mptcp_channel`، وهو ليس MPTCP/SCTP حقيقيًا على مستوى النواة) والتحقق منه عبر اتصال TCP حقيقي عبر loopback (2026-07-13). **④ سجل التدقيق المستقل تم تنفيذه الآن** (`open-web-server-ledger::audit_log::FileAuditLog`، 2026-07-13، ملف إلحاق فقط مع مجموع اختباري SHA-256، مستقل تقنيًا عن WAL/aruaru-db، مع `scan_and_verify()`/`reconcile()`). أما البقية (②aruaru-db، ③النسخ المتزامن متعدد المناطق) فلم تبدأ بعد (التفاصيل: [README-Japan.md](README-Japan.md#6-目標アーキテクチャ-通信層dbの四重化)، [CLAUDE.md](CLAUDE.md)). **التطوير الجديد المخطط له تاليًا**: ربط التزامات aruaru-db بلقطات ZFS (open-raid-z) — لم يُعثر على تقنية راسخة، لكنها تُعتبر اكتشافًا جديدًا وقابلاً للتنفيذ، ومخطط له في مرحلة قادمة.

6. **تقديم الملفات الثابتة + PHP** (`static_files`/`php_server`/`web_vhost`، 2026-07-20) — الخطوة الأولى نحو محرك تسليم هجين على غرار Apache+Nginx. يُربط كل اسم مضيف بمجلد جذر (docroot)؛ المسارات التي يمكن التعرف عليها كأصول ثابتة عبر الامتداد تُقدَّم مباشرة من القرص (مع الحماية من هجمات path traversal)، بينما يُعاد توجيه الباقي إلى عملية فرعية `php -S` تُشغَّل عند الحاجة. جرى التحقق من ذلك بتقديم موقع `audiocafe.tokyo` (موقع PHP قائم) فعليًا.

## البدء السريع

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1
cargo run -p open-runo-gateway
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## البنية (4 حزم)

`open-web-server-core` (نماذج المجال/الأخطاء)، `open-web-server-wire` (النقل الدفاعي رباعي الطبقات)،
`open-web-server-ledger` (WAL متكافئ + التزام عبر 3 قفزات)، `open-web-server-gateway` (بوابة Poem).

## الترخيص

Apache-2.0
