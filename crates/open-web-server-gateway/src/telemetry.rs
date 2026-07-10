//! OpenTelemetry 連携。
//!
//! `tracing` で記録したスパンを OpenTelemetry の Tracer に橋渡しする。
//!
//! - `OTEL_EXPORTER_OTLP_ENDPOINT` 環境変数が設定されている場合は、その
//!   エンドポイントへ OTLP/HTTP (protobuf) でスパンをエクスポートする
//!   (本番/ステージング環境で Collector に接続する想定)。
//! - 未設定の場合は標準出力へスパンを書き出す `opentelemetry-stdout` を
//!   使う (ローカル開発・Collector 未起動時のフォールバック)。
//!
//! いずれの場合も `open-runo`/`aruaru-db` 側から見た「aruaru-web ->
//! open-web-server -> open-runo -> aruaru-db」の一連のリクエストを
//! 分散トレースとして追跡できるようにするための土台。GraphQL 化や
//! `open-cosmo` 共通クレート抽出時にも、この初期化ロジックはそのまま
//! 再利用できる想定で `open-web-server-gateway` バイナリから独立した
//! モジュールとして切り出してある。

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// サービス名 (OpenTelemetry Resource の `service.name` 属性に使う)。
pub const SERVICE_NAME: &str = "open-web-server";

/// 初期化後にプロセス終了時 `shutdown()` を呼ぶためのハンドル。
///
/// `SdkTracerProvider` はバッファリングした未送信スパンを持つため、
/// プロセス終了前に明示的に `shutdown()` してフラッシュする必要がある。
pub struct TelemetryGuard {
    provider: SdkTracerProvider,
}

impl TelemetryGuard {
    /// バッファ済みスパンをフラッシュしてエクスポータをシャットダウンする。
    pub fn shutdown(&self) {
        if let Err(e) = self.provider.shutdown() {
            eprintln!("otel tracer provider shutdown failed: {e}");
        }
    }
}

/// リソース (サービスメタデータ) を構築する。
fn resource() -> Resource {
    Resource::builder()
        .with_attributes(vec![
            KeyValue::new("service.name", SERVICE_NAME),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ])
        .build()
}

/// `OTEL_EXPORTER_OTLP_ENDPOINT` の有無に応じてエクスポータを選び、
/// `SdkTracerProvider` を構築する。
fn build_tracer_provider() -> anyhow::Result<SdkTracerProvider> {
    let builder = SdkTracerProvider::builder().with_resource(resource());

    let provider = if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .build()?;
        builder.with_batch_exporter(exporter).build()
    } else {
        // Collector 未設定時は標準出力にスパンを書き出す (開発用フォールバック)。
        let exporter = opentelemetry_stdout::SpanExporter::default();
        builder.with_batch_exporter(exporter).build()
    };

    Ok(provider)
}

/// `tracing` サブスクライバを初期化し、OpenTelemetry へスパンを橋渡しする
/// レイヤーと、人間可読な `fmt` レイヤーの両方を登録する。
///
/// 戻り値の [`TelemetryGuard`] はプロセス終了直前 (`main` の末尾) で
/// `shutdown()` を呼び出し、バッファ済みスパンを確実にエクスポートすること。
pub fn init() -> anyhow::Result<TelemetryGuard> {
    let provider = build_tracer_provider()?;
    let tracer = provider.tracer(SERVICE_NAME);

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let fmt_layer = tracing_subscriber::fmt::layer();
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(otel_layer)
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to install tracing subscriber: {e}"))?;

    Ok(TelemetryGuard { provider })
}

#[cfg(test)]
mod tests {
    use opentelemetry::trace::{Tracer, TracerProvider as _};
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};

    /// OpenTelemetry の配線そのもの (Resource属性、スパン生成、エクスポート)
    /// が実際に機能することを、実際のネットワーク送信を伴わないインメモリ
    /// エクスポータで検証する。`init()` が使う `build_tracer_provider` は
    /// 環境変数分岐のみを担い、実際のスパン生成/属性付与ロジックはここで
    /// 検証する構造 (`SdkTracerProvider` + exporter の組み合わせ) と同一である。
    #[test]
    fn spans_are_recorded_and_exported_with_service_resource() {
        // resource() が持つ service.name 属性がリソースに正しく設定されている。
        let resource = super::resource();
        assert_eq!(
            resource
                .get(&opentelemetry::Key::new("service.name"))
                .map(|v| v.to_string()),
            Some(super::SERVICE_NAME.to_string())
        );

        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_resource(resource)
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = provider.tracer(super::SERVICE_NAME);

        tracer.in_span("grant_item", |_cx| {
            // 実際のハンドラでも `#[tracing::instrument]` によって
            // 同様の子スパンが記録される。
        });

        provider.force_flush().expect("flush should succeed");

        let spans = exporter.get_finished_spans().expect("exporter readable");
        assert_eq!(spans.len(), 1, "expected exactly one exported span");
        assert_eq!(spans[0].name, "grant_item");

        provider.shutdown().expect("shutdown should succeed");
    }
}
