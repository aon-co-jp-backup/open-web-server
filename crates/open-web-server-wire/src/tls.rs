//! 第1層: 伝送路暗号化 (TLS 1.3 / rustls)

use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
    sync::RwLock,
};

use rustls::sign::CertifiedKey;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

#[derive(Debug, Clone)]
pub struct TlsServerConfig {
    pub cert_path: String,
    pub key_path: String,
}

impl TlsServerConfig {
    pub fn load(&self) -> anyhow::Result<Arc<rustls::ServerConfig>> {
        let certs = load_certs(&self.cert_path)?;
        let key = load_key(&self.key_path)?;

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        Ok(Arc::new(config))
    }
}

fn load_certs(path: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let file = File::open(Path::new(path))?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    Ok(certs)
}

fn load_key(path: &str) -> anyhow::Result<PrivateKeyDer<'static>> {
    let file = File::open(Path::new(path))?;
    let mut reader = BufReader::new(file);
    let key = rustls_pemfile::private_key(&mut reader)?
        .ok_or_else(|| anyhow::anyhow!("no private key found at {path}"))?;
    Ok(key)
}

fn parse_cert_chain(pem: &[u8]) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(pem);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        anyhow::bail!("no certificates found in PEM input");
    }
    Ok(certs)
}

fn parse_private_key(pem: &[u8]) -> anyhow::Result<PrivateKeyDer<'static>> {
    let mut reader = BufReader::new(pem);
    rustls_pemfile::private_key(&mut reader)?.ok_or_else(|| anyhow::anyhow!("no private key found in PEM input"))
}

fn cert_file_path(dir: &Path, host: &str) -> PathBuf {
    dir.join(format!("{host}.pem"))
}

fn key_file_path(dir: &Path, host: &str) -> PathBuf {
    dir.join(format!("{host}.key"))
}

/// `bytes`を`path`へアトミックに書き込む(同一ディレクトリ内へ一時ファイル
/// を書いてから`rename`する、`KeyGuardian::write_records_atomically`
/// (`crates/open-web-server-gateway/src/keyring.rs`)と同じパターン)。
/// プロセスが書き込みの途中で強制終了しても、`rename`はPOSIX/Windowsの
/// いずれでも単一のファイルシステム操作として不可分に行われるため、
/// 既存の完全なファイルが半端な内容で上書きされることはない
/// (一時ファイル自体が不完全な場合はrename前でtemp側に留まり、既存の
/// 本ファイルには一切影響しない)。
fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let tmp_path = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or("tmp"),
        std::process::id()
    ));
    std::fs::write(&tmp_path, bytes)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// `host`向けの証明書チェーン+秘密鍵PEMを`<dir>/<host>.pem`+
/// `<dir>/<host>.key`へそれぞれアトミックに書き込む。
fn persist_cert_to_disk(dir: &Path, host: &str, cert_chain_pem: &[u8], key_pem: &[u8]) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    write_atomically(&cert_file_path(dir, host), cert_chain_pem)?;
    write_atomically(&key_file_path(dir, host), key_pem)?;
    Ok(())
}

/// ディスク上の`cert_path`/`key_path`のPEMファイルペアを読み込み、
/// `CertifiedKey`へ変換する(`load_from_disk`の起動時ロード用)。
/// 破損/不完全なファイル(書き込み中断等)は`Err`を返し、呼び出し側が
/// その1ホストだけをスキップできるようにする(サーバー全体をパニック
/// させない)。
fn load_certified_key_from_files(cert_path: &Path, key_path: &Path) -> anyhow::Result<Arc<CertifiedKey>> {
    let cert_chain_pem = std::fs::read(cert_path)?;
    let key_pem = std::fs::read(key_path)?;
    let chain = parse_cert_chain(&cert_chain_pem)?;
    let key = parse_private_key(&key_pem)?;
    let signing_key = rustls::crypto::ring::sign::any_supported_type(&key)
        .map_err(|e| anyhow::anyhow!("unsupported private key: {e}"))?;
    Ok(Arc::new(CertifiedKey::new(chain, signing_key)))
}

/// rustlsのCryptoProvider(ring)をプロセス内で一度だけインストールする
/// (`quic_channel.rs`の同名ヘルパーと同じ理由・同じ実装 — rustls 0.23は
/// 複数のcrypto backendがfeatureとして有効な場合に備え、プロセス全体で
/// 使うデフォルトを明示する必要がある)。`ServerConfig::builder()`/
/// `ClientConfig::builder()`(引数無し版)はこれが未インストールだと
/// パニックするため、これらを呼ぶ前に必ず呼び出すこと。
fn ensure_crypto_provider_installed() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// SNI(ClientHelloのserver_name)に応じて、テナント(ドメイン)ごとに別々の
/// 証明書を返す `ResolvesServerCert` 実装。これが無いと、open-web-server
/// 自体は1プロセスにつき1証明書しか提供できず(既存の`TlsServerConfig`)、
/// `tenant_router::TenantRegistry`が既に実現している「1プロセスで複数
/// ドメインを動的に振り分ける」というマルチテナント運用を、TLS終端の面
/// では実現できていなかった——本リゾルバがその欠落を埋める。
///
/// 実世界の同種実装(rustls上で複数ドメインをTLS終端するリバースプロキシ
/// `rpxy`等)と同じ、`rustls::server::ResolvesServerCert` + ホスト名ごとの
/// `CertifiedKey`辞書という標準パターンに沿う(2026-07-16、EN/JP両言語で
/// 実務例を調査済み)。
#[derive(Debug, Default)]
pub struct TenantCertResolver {
    certs: RwLock<HashMap<String, Arc<CertifiedKey>>>,
    /// 取得済みACME証明書をプロセス再起動を跨いで生存させるためのディスク
    /// 保存先(`OPEN_WEB_SERVER_TLS_CERT_DIR`、2026-07-26追記)。`None`なら
    /// 従来通りプロセス内メモリのみ(既存動作を一切変えない後方互換)。
    ///
    /// **背景(実際に発生した本番障害)**: このフィールドが無かった旧実装は
    /// 証明書をメモリにしか保持せず、ルーティング設定変更などの理由で
    /// `open-web-server`サービスを再起動しただけで、稼働中の約20ドメイン
    /// 全てのTLS証明書が消え、HTTPS経由の全アクセスが一斉に落ちた。
    /// うち1ドメイン(`karu.tokyo`)は短期間に何度も再起動して証明書を
    /// 再取得したため、Let's Encryptの実レート制限
    /// ("too many certificates (5) already issued for this exact set of
    /// identifiers in the last 168h0m0s")に到達し、約24時間有効な証明書を
    /// 取得できないまま停止する事態になった。本フィールド以降の永続化・
    /// 起動時ロードは、この障害の直接の再発防止策(regression fix)である。
    cert_dir: Option<PathBuf>,
}

impl TenantCertResolver {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// `cert_dir`(`OPEN_WEB_SERVER_TLS_CERT_DIR`)配下に既に存在する
    /// `<host>.pem`(証明書チェーン)+`<host>.key`(秘密鍵)ペアを起動時に
    /// すべて読み込み、以後の`upsert_pem`呼び出しはこのディレクトリへも
    /// アトミックに書き込むようにする。これにより、プロセス再起動後も
    /// ACMEで取得済みだった証明書がゼロ回のACME呼び出しで即座に有効になる
    /// (本モジュールdocの「実際に発生した本番障害」の再発防止策)。
    ///
    /// **部分的/破損したファイルへの耐性**: 個々のホストの`.pem`/`.key`が
    /// 読めない・パースできない(書き込み中断による不完全なファイル等)
    /// 場合でも、その1ホストをスキップして警告ログを出すのみで、サーバー
    /// 全体の起動をパニックさせたり他のホストの読み込みをブロックしたり
    /// しない(既存の`KeyGuardian::load_from_disk`/ACMEアカウント鍵永続化と
    /// 同じ「補助的な永続化の失敗は権威パスを止めない」設計方針)。
    pub fn load_from_disk(cert_dir: PathBuf) -> Arc<Self> {
        let mut certs = HashMap::new();
        match std::fs::read_dir(&cert_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("pem") {
                        continue;
                    }
                    let Some(host) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let key_path = path.with_extension("key");
                    match load_certified_key_from_files(&path, &key_path) {
                        Ok(certified_key) => {
                            tracing::info!(host, "TenantCertResolver: loaded persisted TLS certificate from disk");
                            certs.insert(host.to_ascii_lowercase(), certified_key);
                        }
                        Err(e) => {
                            tracing::warn!(
                                host,
                                error = %e,
                                cert_path = %path.display(),
                                "TenantCertResolver: skipping unreadable/corrupt persisted certificate for host"
                            );
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(cert_dir = %cert_dir.display(), "TenantCertResolver: cert dir does not exist yet, starting empty");
            }
            Err(e) => {
                tracing::warn!(error = %e, cert_dir = %cert_dir.display(), "TenantCertResolver: failed to read cert dir, starting empty");
            }
        }
        Arc::new(Self { certs: RwLock::new(certs), cert_dir: Some(cert_dir) })
    }

    /// `host`(SNI名、大文字小文字は無視)にPEM形式の証明書チェーン+秘密鍵を
    /// 登録する。既存の登録は上書きされる(証明書更新・ACME再発行後の
    /// ローテーションに使う)。
    pub fn upsert_pem(&self, host: &str, cert_chain_pem: &[u8], key_pem: &[u8]) -> anyhow::Result<()> {
        let chain = parse_cert_chain(cert_chain_pem)?;
        let key = parse_private_key(key_pem)?;
        let signing_key = rustls::crypto::ring::sign::any_supported_type(&key)
            .map_err(|e| anyhow::anyhow!("unsupported private key for {host}: {e}"))?;
        let certified_key = Arc::new(CertifiedKey::new(chain, signing_key));
        let host_key = host.to_ascii_lowercase();
        self.certs
            .write()
            .map_err(|_| anyhow::anyhow!("TenantCertResolver lock poisoned"))?
            .insert(host_key.clone(), certified_key);

        // ディスク永続化(`cert_dir`が設定されている場合のみ)。書き込み失敗は
        // 警告ログのみでこの呼び出し自体は成功として扱う——メモリ内には既に
        // 反映済みであり、TLS終端は直ちに機能する。永続化はあくまで「次回
        // 再起動でもゼロ回のACME呼び出しで復元できる」という耐障害性の
        // 上乗せであり、これの失敗で証明書登録全体を失敗させるのは
        // 過剰反応(補助系の失敗は権威パスをブロックしない、既存方針通り)。
        if let Some(dir) = &self.cert_dir {
            if let Err(e) = persist_cert_to_disk(dir, &host_key, cert_chain_pem, key_pem) {
                tracing::warn!(
                    host = %host_key,
                    error = %e,
                    cert_dir = %dir.display(),
                    "TenantCertResolver: failed to persist TLS certificate to disk (in-memory registration still succeeded)"
                );
            }
        }
        Ok(())
    }

    /// `host`(PEMファイルパス版)。ACME自動更新やvhost追加時、ディスク上の
    /// 証明書ファイルからそのまま登録したい場合の薄いラッパー。
    pub fn upsert_from_files(&self, host: &str, cert_path: &str, key_path: &str) -> anyhow::Result<()> {
        let cert_pem = std::fs::read(cert_path)?;
        let key_pem = std::fs::read(key_path)?;
        self.upsert_pem(host, &cert_pem, &key_pem)
    }

    /// `host`の証明書登録を削除する(テナント削除時、Apacheの`a2dissite`
    /// 相当)。登録が無かった場合も静かに成功する(冪等)。
    pub fn remove(&self, host: &str) -> anyhow::Result<()> {
        let host_key = host.to_ascii_lowercase();
        self.certs
            .write()
            .map_err(|_| anyhow::anyhow!("TenantCertResolver lock poisoned"))?
            .remove(&host_key);
        if let Some(dir) = &self.cert_dir {
            let _ = std::fs::remove_file(cert_file_path(dir, &host_key));
            let _ = std::fs::remove_file(key_file_path(dir, &host_key));
        }
        Ok(())
    }

    pub fn contains(&self, host: &str) -> bool {
        self.certs
            .read()
            .map(|map| map.contains_key(&host.to_ascii_lowercase()))
            .unwrap_or(false)
    }
}

impl rustls::server::ResolvesServerCert for TenantCertResolver {
    fn resolve(&self, client_hello: rustls::server::ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let server_name = client_hello.server_name()?;
        self.certs.read().ok()?.get(&server_name.to_ascii_lowercase()).cloned()
    }
}

/// `TenantCertResolver`をSNIに応じた証明書選択に使う`ServerConfig`を組み立てる。
/// クライアント証明書認証は行わない(このアプリの認証はHTTP層のAPIキー/
/// テナント振り分けであり、TLS層のmTLSは既存の`open-web-server-wire`の
/// バックエンド間4層防御通信の方に別途ある——ここは公開向けの通常TLS)。
pub fn build_tenant_server_config(resolver: Arc<TenantCertResolver>) -> Arc<rustls::ServerConfig> {
    ensure_crypto_provider_installed();
    Arc::new(rustls::ServerConfig::builder().with_no_client_auth().with_cert_resolver(resolver))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `rcgen`で使い捨ての自己署名証明書(PEM)を1組生成する。ディスクへの
    /// 書き込みは行わない(このテストは`upsert_pem`のインメモリ経路のみを
    /// 検証する——`upsert_from_files`はこの関数の薄いラッパーなので
    /// 別途のファイルI/Oテストは不要と判断)。
    fn self_signed_pem(subject_alt_name: &str) -> (Vec<u8>, Vec<u8>) {
        let cert = rcgen::generate_simple_self_signed(vec![subject_alt_name.to_string()]).unwrap();
        (cert.cert.pem().into_bytes(), cert.key_pair.serialize_pem().into_bytes())
    }

    #[test]
    fn upsert_then_resolve_returns_none_for_unknown_host() {
        let resolver = TenantCertResolver::new();
        let (cert_pem, key_pem) = self_signed_pem("tenant-a.example.test");
        resolver.upsert_pem("tenant-a.example.test", &cert_pem, &key_pem).unwrap();

        assert!(resolver.contains("tenant-a.example.test"));
        assert!(!resolver.contains("unknown-host.example.test"));
    }

    #[test]
    fn upsert_is_case_insensitive_and_remove_is_idempotent() {
        let resolver = TenantCertResolver::new();
        let (cert_pem, key_pem) = self_signed_pem("Tenant-B.example.test");
        resolver.upsert_pem("Tenant-B.example.test", &cert_pem, &key_pem).unwrap();

        assert!(resolver.contains("tenant-b.example.test"));
        resolver.remove("TENANT-B.example.test").unwrap();
        assert!(!resolver.contains("tenant-b.example.test"));
        // Removing again (already absent) must not error -- idempotent, like
        // the existing `tenant_router::remove` semantics this mirrors.
        resolver.remove("tenant-b.example.test").unwrap();
    }

    #[test]
    fn upsert_rejects_malformed_pem() {
        let resolver = TenantCertResolver::new();
        assert!(resolver.upsert_pem("bad.example.test", b"not a certificate", b"not a key").is_err());
    }

    /// テスト専用: 一意な一時ディレクトリパスを作る(`tempfile`クレートは
    /// このワークスペースの依存に存在しないため、`keyring.rs`のテストと
    /// 同じ手動実装パターンを踏襲、新規依存追加を避ける)。
    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = format!(
            "open-web-server-tls-cert-dir-test-{label}-{}-{}",
            std::process::id(),
            uuid_like_suffix()
        );
        std::env::temp_dir().join(unique)
    }

    /// `uuid`クレートは本ワークスペースの依存に無いため、時刻+アドレスを
    /// 元にした簡易な一意サフィックスを生成する(テスト専用、暗号学的な
    /// 一意性は不要——同一プロセス内でのパス衝突回避が目的)。
    fn uuid_like_suffix() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let addr = &nanos as *const _ as usize;
        format!("{nanos:x}-{addr:x}")
    }

    /// **本番障害の直接の回帰テスト(regression test)**: `upsert_pem`で
    /// ホストの証明書を登録したあと、同じディスクディレクトリを指す
    /// **新規の**`TenantCertResolver`インスタンス(=プロセス再起動を模す)
    /// を構築し、`resolve()`が一切のACME呼び出し無しに即座にその証明書を
    /// 見つけられることを確認する。これが無かった旧実装では、プロセス
    /// 再起動のたびに全ドメインのTLS証明書がメモリから消え、実際に
    /// ~20ドメインが一斉にHTTPS応答不能になった(詳細は`cert_dir`
    /// フィールドのdoc comment参照)。
    #[test]
    fn cert_persisted_via_upsert_pem_survives_simulated_process_restart() {
        let dir = unique_temp_dir("restart-sim");

        // --- "旧プロセス": 証明書を取得してディスクへ永続化する ---
        let resolver_before_restart = TenantCertResolver::load_from_disk(dir.clone());
        let (cert_pem, key_pem) = self_signed_pem("restart-sim.example.test");
        resolver_before_restart
            .upsert_pem("restart-sim.example.test", &cert_pem, &key_pem)
            .unwrap();
        assert!(resolver_before_restart.contains("restart-sim.example.test"));

        // --- プロセス再起動を模す: 元のインスタンスを破棄し、同じ
        //     ディスクディレクトリを指す全く新しいインスタンスを作る ---
        drop(resolver_before_restart);
        let resolver_after_restart = TenantCertResolver::load_from_disk(dir.clone());

        // ACMEを一切呼ばずに、再起動直後から証明書が即座に使える。
        assert!(
            resolver_after_restart.contains("restart-sim.example.test"),
            "certificate must survive a simulated process restart without any ACME call"
        );

        // `resolve()`(rustlsが実際に呼ぶ経路)自体も、生き残った証明書を
        // 正しいバイト列で返すことを確認する(`contains`だけでは
        // 辞書に「何らかの値」があることしか分からないため)。
        let expected_leaf = first_cert_der(&cert_pem);
        let resolved = resolver_after_restart
            .certs
            .read()
            .unwrap()
            .get("restart-sim.example.test")
            .cloned()
            .expect("resolver must have the restored certified key");
        assert_eq!(resolved.cert[0].as_ref(), expected_leaf.as_slice());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 壊れた/不完全なファイル(書き込み中断を模す)が1ホスト分あっても、
    /// そのホストだけがスキップされ、他の正常なホストは引き続き読み込まれ、
    /// サーバー全体がパニックしないことを確認する。
    #[test]
    fn load_from_disk_skips_corrupt_host_and_still_loads_valid_host() {
        let dir = unique_temp_dir("corrupt-skip");
        std::fs::create_dir_all(&dir).unwrap();

        // 正常なホスト。
        let (good_cert_pem, good_key_pem) = self_signed_pem("good-host.example.test");
        std::fs::write(dir.join("good-host.example.test.pem"), &good_cert_pem).unwrap();
        std::fs::write(dir.join("good-host.example.test.key"), &good_key_pem).unwrap();

        // 壊れたホスト: 証明書ファイルの中身がでたらめ(書き込み中断や
        // ディスク破損を模す)。鍵ファイルは意図的に用意しない場合もある。
        std::fs::write(dir.join("corrupt-host.example.test.pem"), b"not actually a pem certificate").unwrap();
        std::fs::write(dir.join("corrupt-host.example.test.key"), b"not actually a pem key either").unwrap();

        let resolver = TenantCertResolver::load_from_disk(dir.clone());

        assert!(resolver.contains("good-host.example.test"), "valid host must still load");
        assert!(!resolver.contains("corrupt-host.example.test"), "corrupt host must be skipped, not crash the loader");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 証明書を`remove()`すると、ディスク上のファイルも実際に削除され、
    /// 次回の再起動(=新規インスタンスでの`load_from_disk`)で復活しない
    /// ことを確認する。
    #[test]
    fn remove_deletes_persisted_files_so_they_do_not_resurrect_on_restart() {
        let dir = unique_temp_dir("remove-then-restart");
        let resolver = TenantCertResolver::load_from_disk(dir.clone());
        let (cert_pem, key_pem) = self_signed_pem("removed-host.example.test");
        resolver.upsert_pem("removed-host.example.test", &cert_pem, &key_pem).unwrap();
        assert!(dir.join("removed-host.example.test.pem").exists());

        resolver.remove("removed-host.example.test").unwrap();
        assert!(!dir.join("removed-host.example.test.pem").exists());
        assert!(!dir.join("removed-host.example.test.key").exists());

        let resolver_after_restart = TenantCertResolver::load_from_disk(dir.clone());
        assert!(!resolver_after_restart.contains("removed-host.example.test"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// これが本テストモジュールの核心: 同一プロセス/同一`ServerConfig`が、
    /// SNIサーバー名だけを見て2つの異なるテナントに別々の証明書を実際に
    /// 返すことを、本物のTLSハンドシェイク(実TCPループバック)で証明する。
    /// 単体テストレベルの「辞書に入っているか」の確認(上記2件)だけでは、
    /// `ResolvesServerCert`の実装がrustls自体から正しく呼ばれる配線に
    /// なっているかまでは検証できないため、これを別途実施する。
    // Test-only verifier: records whatever leaf certificate the server
    // presented and accepts it unconditionally. Production code never uses
    // this -- it exists solely so this test can complete a real TLS 1.3
    // handshake against a self-signed cert without a trust anchor, while
    // still letting the test assert on which cert bytes came back.
    #[derive(Debug, Default)]
    struct RecordingVerifier {
        leaf_der: std::sync::Mutex<Option<Vec<u8>>>,
    }
    impl rustls::client::danger::ServerCertVerifier for RecordingVerifier {
        fn verify_server_cert(
            &self,
            end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            *self.leaf_der.lock().unwrap() = Some(end_entity.to_vec());
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
        }
    }

    async fn handshake_and_get_leaf_cert(
        listener: tokio::net::TcpListener,
        acceptor: tokio_rustls::TlsAcceptor,
        sni: &'static str,
    ) -> Vec<u8> {
        use rustls::pki_types::ServerName;
        use tokio::net::TcpStream;
        use tokio_rustls::TlsConnector;

        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = acceptor.accept(stream).await.unwrap();
        });

        ensure_crypto_provider_installed();
        let verifier = Arc::new(RecordingVerifier::default());
        let client_config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(client_config));

        let tcp = TcpStream::connect(addr).await.unwrap();
        let server_name = ServerName::try_from(sni).unwrap();
        let _tls_stream = connector.connect(server_name, tcp).await.unwrap();
        server_task.await.unwrap();

        let leaf = verifier.leaf_der.lock().unwrap().clone().unwrap();
        leaf
    }

    fn first_cert_der(pem: &[u8]) -> Vec<u8> {
        let mut reader = BufReader::new(pem);
        let der = rustls_pemfile::certs(&mut reader).next().unwrap().unwrap().to_vec();
        der
    }

    /// これが本テストモジュールの核心: 同一プロセス/同一`ServerConfig`が、
    /// SNIサーバー名だけを見て2つの異なるテナントに別々の証明書を実際に
    /// 返すことを、本物のTLSハンドシェイク(実TCPループバック)で証明する。
    /// 単体テストレベルの「辞書に入っているか」の確認(上記2件)だけでは、
    /// `ResolvesServerCert`の実装がrustls自体から正しく呼ばれる配線に
    /// なっているかまでは検証できないため、これを別途実施する。
    #[tokio::test]
    async fn real_tls_handshake_resolves_different_cert_per_sni() {
        use tokio::net::TcpListener;
        use tokio_rustls::TlsAcceptor;

        let resolver = TenantCertResolver::new();
        let (cert_a_pem, key_a_pem) = self_signed_pem("tenant-a.example.test");
        let (cert_b_pem, key_b_pem) = self_signed_pem("tenant-b.example.test");
        resolver.upsert_pem("tenant-a.example.test", &cert_a_pem, &key_a_pem).unwrap();
        resolver.upsert_pem("tenant-b.example.test", &cert_b_pem, &key_b_pem).unwrap();

        let server_config = build_tenant_server_config(resolver);
        let acceptor = TlsAcceptor::from(server_config);

        // Each handshake gets its own freshly-bound ephemeral-port listener
        // (rather than reusing one address across two sequential accepts),
        // so there's no risk of a port-reuse race between the two handshakes.
        let listener_a = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let leaf_a = handshake_and_get_leaf_cert(listener_a, acceptor.clone(), "tenant-a.example.test").await;

        let listener_b = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let leaf_b = handshake_and_get_leaf_cert(listener_b, acceptor, "tenant-b.example.test").await;

        assert_ne!(leaf_a, leaf_b, "different SNI names must resolve to different certificates");
        assert_eq!(leaf_a, first_cert_der(&cert_a_pem));
        assert_eq!(leaf_b, first_cert_der(&cert_b_pem));
    }
}
