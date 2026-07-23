//! 組み込みSFTPサーバー(`sftp` feature、既定オフ)。
//!
//! 固定IPを持たない自宅サーバー等でも、`open-web-server`プロセス自体が
//! SFTPサーバー機能を持つことで、外部の`sshd`/`vsftpd`等への依存を増やさず
//! 「単一バイナリで完結する」という既存の設計思想(`ddns.rs`/`acme.rs`と
//! 同じ)に合わせる。`russh`(pure-Rust SSHサーバー実装)+ `russh-sftp`
//! (SFTPサブシステム)を使う。
//!
//! # 有効化
//! `OPEN_WEB_SERVER_SFTP_BIND`(例: `0.0.0.0:2222`)を設定した場合のみ起動する
//! (未設定なら何もしない、既存のTLS/DDNSと同じ「オプトイン」設計)。
//!
//! # 認証
//! 公開鍵認証を基本とし、`OPEN_WEB_SERVER_SFTP_AUTHORIZED_KEYS_FILE`
//! (OpenSSH形式の`authorized_keys`)で許可鍵を管理する。パスワード認証は
//! セキュリティ上の既定オフとし、`OPEN_WEB_SERVER_SFTP_ALLOW_PASSWORD_AUTH=true`
//! + `OPEN_WEB_SERVER_SFTP_PASSWORD`で明示opt-inできる(平文比較ではなく
//! 定数時間比較、ただし公開鍵認証の方が強く推奨される)。
//!
//! # ルートディレクトリ・パストラバーサル対策
//! `OPEN_WEB_SERVER_SFTP_ROOT`(既定は `./sftp-root`)配下に閉じ込める。
//! `static_files.rs`の`canonicalize()` + `starts_with`パターンと同じ方針を
//! ここでも踏襲する(`resolve_within_root`参照)。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh::keys::key::{KeyPair, PublicKey};
use russh::server::{Auth, Handler, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};
use tokio::sync::Mutex;

/// `authorized_keys`(OpenSSH形式)をパースして、許可された公開鍵の集合を返す。
/// 各行はコメント込みのフル行(`ssh-ed25519 AAAA... comment`)を許容する。
pub fn load_authorized_keys(path: &Path) -> anyhow::Result<Vec<PublicKey>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read authorized_keys file '{}': {e}", path.display()))?;
    let mut keys = Vec::new();
    for (lineno, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // OpenSSH形式は `<algo> <base64> [comment]`。base64フィールドのみを
        // `parse_public_key_base64`へ渡す。
        let Some(base64_field) = line.split_whitespace().nth(1) else {
            tracing::warn!(lineno = lineno + 1, "skipping malformed authorized_keys line (missing base64 field)");
            continue;
        };
        match russh::keys::parse_public_key_base64(base64_field) {
            Ok(key) => keys.push(key),
            Err(e) => {
                tracing::warn!(lineno = lineno + 1, error = %e, "skipping unparsable authorized_keys line");
            }
        }
    }
    Ok(keys)
}

/// `OPEN_WEB_SERVER_SFTP_BIND` が設定されていればSFTPサーバーを起動する。
/// 未設定なら何もしない(既存のTLS/DDNSと同じ「オプトイン」設計)。
pub fn spawn_if_configured() {
    let Ok(bind_addr) = std::env::var("OPEN_WEB_SERVER_SFTP_BIND") else {
        return;
    };

    let root = std::env::var("OPEN_WEB_SERVER_SFTP_ROOT").unwrap_or_else(|_| "./sftp-root".to_string());
    let root = PathBuf::from(root);
    if let Err(e) = std::fs::create_dir_all(&root) {
        tracing::error!(error = %e, root = %root.display(), "failed to create SFTP root directory; SFTP server not started");
        return;
    }
    let root = match root.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "failed to canonicalize SFTP root; SFTP server not started");
            return;
        }
    };

    let authorized_keys = match std::env::var("OPEN_WEB_SERVER_SFTP_AUTHORIZED_KEYS_FILE") {
        Ok(path) => match load_authorized_keys(Path::new(&path)) {
            Ok(keys) => keys,
            Err(e) => {
                tracing::error!(error = %e, "failed to load authorized_keys; SFTP server not started");
                return;
            }
        },
        Err(_) => Vec::new(),
    };

    let allow_password = std::env::var("OPEN_WEB_SERVER_SFTP_ALLOW_PASSWORD_AUTH")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let password = std::env::var("OPEN_WEB_SERVER_SFTP_PASSWORD").ok();

    if authorized_keys.is_empty() && !(allow_password && password.is_some()) {
        tracing::warn!(
            "OPEN_WEB_SERVER_SFTP_BIND is set but no authorized_keys and no opted-in password are configured; \
             the SFTP server will start but no client will be able to authenticate"
        );
    }

    tokio::spawn(async move {
        if let Err(e) = run(bind_addr, root, authorized_keys, allow_password, password).await {
            tracing::error!(error = %e, "SFTP server exited with error");
        }
    });
}

async fn run(
    bind_addr: String,
    root: PathBuf,
    authorized_keys: Vec<PublicKey>,
    allow_password: bool,
    password: Option<String>,
) -> anyhow::Result<()> {
    let keypair = KeyPair::generate_ed25519()
        .ok_or_else(|| anyhow::anyhow!("failed to generate SFTP host key (ed25519 keygen failed)"))?;

    let config = russh::server::Config {
        auth_rejection_time: std::time::Duration::from_secs(1),
        keys: vec![keypair],
        ..Default::default()
    };

    let mut server = SftpAppServer {
        root: Arc::new(root.clone()),
        authorized_keys: Arc::new(authorized_keys),
        allow_password,
        password: Arc::new(password),
    };

    tracing::info!(%bind_addr, root = %root.display(), "open-web-server embedded SFTP server listening");
    server.run_on_address(Arc::new(config), bind_addr.as_str()).await?;
    Ok(())
}

#[derive(Clone)]
struct SftpAppServer {
    root: Arc<PathBuf>,
    authorized_keys: Arc<Vec<PublicKey>>,
    allow_password: bool,
    password: Arc<Option<String>>,
}

impl russh::server::Server for SftpAppServer {
    type Handler = SshSession;

    fn new_client(&mut self, _peer_addr: Option<std::net::SocketAddr>) -> SshSession {
        SshSession {
            root: self.root.clone(),
            authorized_keys: self.authorized_keys.clone(),
            allow_password: self.allow_password,
            password: self.password.clone(),
        }
    }
}

struct SshSession {
    root: Arc<PathBuf>,
    authorized_keys: Arc<Vec<PublicKey>>,
    allow_password: bool,
    password: Arc<Option<String>>,
}

#[async_trait::async_trait]
impl Handler for SshSession {
    type Error = russh::Error;

    async fn auth_publickey(&mut self, _user: &str, key: &PublicKey) -> Result<Auth, Self::Error> {
        if self.authorized_keys.iter().any(|allowed| allowed == key) {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::Reject { proceed_with_methods: None })
        }
    }

    async fn auth_password(&mut self, _user: &str, provided: &str) -> Result<Auth, Self::Error> {
        if !self.allow_password {
            return Ok(Auth::Reject { proceed_with_methods: None });
        }
        match self.password.as_ref() {
            Some(expected) if subtle_eq(expected.as_bytes(), provided.as_bytes()) => Ok(Auth::Accept),
            _ => Ok(Auth::Reject { proceed_with_methods: None }),
        }
    }

    async fn channel_open_session(&mut self, channel: Channel<Msg>, _session: &mut Session) -> Result<bool, Self::Error> {
        let handler = SftpSubsystem::new(self.root.as_ref().clone());
        let stream = channel.into_stream();
        tokio::spawn(async move {
            russh_sftp::server::run(stream, handler).await;
        });
        Ok(true)
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name == "sftp" {
            session.channel_success(channel_id);
        } else {
            session.channel_failure(channel_id);
        }
        Ok(())
    }
}

/// 定数時間比較(既存の`open-web-server-wire::subtle`利用パターンに合わせ、
/// タイミング攻撃対策としてパスワード比較にも同種の防御を入れる)。
fn subtle_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// SFTPサブシステムのファイルシステム操作実装。すべてのパスは`root`配下に
/// 限定する(`resolve_within_root`、`static_files.rs`と同じcanonicalize +
/// starts_with方針)。
struct SftpSubsystem {
    root: PathBuf,
    version: Option<u32>,
    handles: Mutex<HashMap<String, PathBuf>>,
    /// `readdir`を読み切ったディレクトリハンドルの集合(`readdir`のdoc
    /// comment参照)。
    exhausted_dirs: Mutex<std::collections::HashSet<String>>,
    next_handle: Mutex<u64>,
}

impl SftpSubsystem {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            version: None,
            handles: Mutex::new(HashMap::new()),
            exhausted_dirs: Mutex::new(std::collections::HashSet::new()),
            next_handle: Mutex::new(0),
        }
    }

    /// リクエストされた相対パス(SFTPプロトコル上のパス、`/`区切り)を、
    /// `root`配下に閉じ込めた実ファイルシステムパスへ解決する。
    /// パストラバーサル(`..`・絶対パスでのroot外指定)は拒否する。
    fn resolve_within_root(&self, requested: &str) -> Result<PathBuf, StatusCode> {
        let trimmed = requested.trim_start_matches('/');
        let candidate = if trimmed.is_empty() {
            self.root.clone()
        } else {
            self.root.join(trimmed)
        };

        // まだ存在しないファイル(アップロード先の新規ファイル等)向けに、
        // 親ディレクトリの正規化で判定する。
        let check_target = if candidate.exists() { candidate.clone() } else {
            match candidate.parent() {
                Some(parent) if parent.exists() => parent.to_path_buf(),
                _ => return Err(StatusCode::NoSuchFile),
            }
        };

        let canonical_root = self.root.canonicalize().map_err(|_| StatusCode::Failure)?;
        let canonical_check = check_target.canonicalize().map_err(|_| StatusCode::NoSuchFile)?;
        if !canonical_check.starts_with(&canonical_root) {
            tracing::warn!(requested, "SFTP path traversal attempt rejected");
            return Err(StatusCode::PermissionDenied);
        }

        Ok(candidate)
    }
}

fn file_attributes_for(path: &Path) -> FileAttributes {
    let mut attrs = FileAttributes::default();
    if let Ok(meta) = std::fs::metadata(path) {
        attrs.size = Some(meta.len());
        attrs.set_dir(meta.is_dir());
        attrs.permissions = Some(if meta.is_dir() { 0o040755 } else { 0o100644 });
    }
    attrs
}

impl russh_sftp::server::Handler for SftpSubsystem {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, version: u32, _extensions: HashMap<String, String>) -> Result<Version, Self::Error> {
        self.version = Some(version);
        Ok(Version::new())
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let resolved = self.resolve_within_root(&path)?;
        if !resolved.is_dir() {
            return Err(StatusCode::NoSuchFile);
        }
        let mut counter = self.next_handle.lock().await;
        *counter += 1;
        let handle_id = format!("dir-{}", *counter);
        self.handles.lock().await.insert(handle_id.clone(), resolved);
        Ok(Handle { id, handle: handle_id })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        // SFTPプロトコルは「1回目のreaddirで全件返す→2回目の呼び出しで
        // Eofを返して終端」という往復を期待する(クライアント側は空リストを
        // 受け取るまで呼び続ける)。ハンドル自体は`close`まで生かしたまま、
        // 「読み切ったかどうか」を別集合で管理する(ハンドルを早期に消すと
        // 2回目の呼び出しが `Failure` になり、実クライアント実装が
        // エラー扱いしてしまうバグを避けるため)。
        if self.exhausted_dirs.lock().await.contains(&handle) {
            return Err(StatusCode::Eof);
        }

        let handles = self.handles.lock().await;
        let Some(dir) = handles.get(&handle) else {
            return Err(StatusCode::Failure);
        };
        let dir = dir.clone();
        drop(handles);

        let entries = std::fs::read_dir(&dir).map_err(|_| StatusCode::Failure)?;
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let attrs = file_attributes_for(&entry.path());
            files.push(russh_sftp::protocol::File { filename: file_name.clone(), longname: file_name, attrs });
        }
        self.exhausted_dirs.lock().await.insert(handle);
        Ok(Name { id, files })
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let resolved = self.resolve_within_root(&path)?;
        let canonical_root = self.root.canonicalize().map_err(|_| StatusCode::Failure)?;
        let display = if resolved == self.root {
            "/".to_string()
        } else {
            let rel = resolved
                .strip_prefix(&canonical_root)
                .or_else(|_| resolved.strip_prefix(&self.root))
                .unwrap_or(&resolved);
            format!("/{}", rel.to_string_lossy().replace('\\', "/"))
        };
        Ok(Name {
            id,
            files: vec![russh_sftp::protocol::File { filename: display.clone(), longname: display, attrs: FileAttributes::default() }],
        })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.stat(id, path).await
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let resolved = self.resolve_within_root(&path)?;
        if !resolved.exists() {
            return Err(StatusCode::NoSuchFile);
        }
        Ok(Attrs { id, attrs: file_attributes_for(&resolved) })
    }

    async fn open(&mut self, id: u32, filename: String, pflags: OpenFlags, _attrs: FileAttributes) -> Result<Handle, Self::Error> {
        let resolved = self.resolve_within_root(&filename)?;
        if pflags.contains(OpenFlags::WRITE) {
            // アップロード用: root配下であることは`resolve_within_root`で確認済み。
        } else if !resolved.exists() {
            return Err(StatusCode::NoSuchFile);
        }
        let mut counter = self.next_handle.lock().await;
        *counter += 1;
        let handle_id = format!("file-{}", *counter);
        self.handles.lock().await.insert(handle_id.clone(), resolved);
        Ok(Handle { id, handle: handle_id })
    }

    async fn read(&mut self, id: u32, handle: String, offset: u64, len: u32) -> Result<Data, Self::Error> {
        let handles = self.handles.lock().await;
        let Some(path) = handles.get(&handle) else {
            return Err(StatusCode::Failure);
        };
        let path = path.clone();
        drop(handles);

        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(&path).map_err(|_| StatusCode::Failure)?;
        file.seek(SeekFrom::Start(offset)).map_err(|_| StatusCode::Failure)?;
        let mut buf = vec![0u8; len as usize];
        let read_bytes = file.read(&mut buf).map_err(|_| StatusCode::Failure)?;
        if read_bytes == 0 {
            return Err(StatusCode::Eof);
        }
        buf.truncate(read_bytes);
        Ok(Data { id, data: buf })
    }

    async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
        let handles = self.handles.lock().await;
        let Some(path) = handles.get(&handle) else {
            return Err(StatusCode::Failure);
        };
        let path = path.clone();
        drop(handles);

        use std::io::{Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)
            .map_err(|_| StatusCode::Failure)?;
        file.seek(SeekFrom::Start(offset)).map_err(|_| StatusCode::Failure)?;
        file.write_all(&data).map_err(|_| StatusCode::Failure)?;
        Ok(Status { id, status_code: StatusCode::Ok, error_message: "ok".into(), language_tag: "en".into() })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.handles.lock().await.remove(&handle);
        self.exhausted_dirs.lock().await.remove(&handle);
        Ok(Status { id, status_code: StatusCode::Ok, error_message: "ok".into(), language_tag: "en".into() })
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        let resolved = self.resolve_within_root(&filename)?;
        std::fs::remove_file(&resolved).map_err(|_| StatusCode::Failure)?;
        Ok(Status { id, status_code: StatusCode::Ok, error_message: "ok".into(), language_tag: "en".into() })
    }

    async fn mkdir(&mut self, id: u32, path: String, _attrs: FileAttributes) -> Result<Status, Self::Error> {
        let resolved = self.resolve_within_root(&path)?;
        std::fs::create_dir(&resolved).map_err(|_| StatusCode::Failure)?;
        Ok(Status { id, status_code: StatusCode::Ok, error_message: "ok".into(), language_tag: "en".into() })
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        let resolved = self.resolve_within_root(&path)?;
        std::fs::remove_dir(&resolved).map_err(|_| StatusCode::Failure)?;
        Ok(Status { id, status_code: StatusCode::Ok, error_message: "ok".into(), language_tag: "en".into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtle_eq_matches_equal_and_rejects_different() {
        assert!(subtle_eq(b"secret", b"secret"));
        assert!(!subtle_eq(b"secret", b"secrxt"));
        assert!(!subtle_eq(b"secret", b"short"));
    }

    #[test]
    fn spawn_if_configured_is_a_noop_without_env_var() {
        std::env::remove_var("OPEN_WEB_SERVER_SFTP_BIND");
        spawn_if_configured();
    }

    #[tokio::test]
    async fn resolve_within_root_rejects_parent_traversal() {
        let tmp = std::env::temp_dir().join(format!("owsftp-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let root = tmp.canonicalize().unwrap();
        let sub = SftpSubsystem::new(root.clone());

        // root配下の通常パスは許可される。
        std::fs::write(root.join("ok.txt"), b"hello").unwrap();
        assert!(sub.resolve_within_root("ok.txt").is_ok());

        // `..`によるroot外への脱出は拒否される。
        let escape_target = root.parent().unwrap().join("owsftp-escape-marker.txt");
        std::fs::write(&escape_target, b"leak").unwrap();
        let result = sub.resolve_within_root("../owsftp-escape-marker.txt");
        assert!(result.is_err(), "path traversal outside SFTP root must be rejected");

        let _ = std::fs::remove_file(&escape_target);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// クライアント側の`check_server_key`を「テストなので何でも信頼する」に
    /// する最小ハンドラ(本番コードはこれを一切使わない、`main.rs`の
    /// `AcceptAnyCert`と同じ位置づけ)。
    struct AcceptAnyServerKey;
    #[async_trait::async_trait]
    impl russh::client::Handler for AcceptAnyServerKey {
        type Error = russh::Error;

        async fn check_server_key(&mut self, _server_public_key: &PublicKey) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    /// エンドツーエンド検証: 実SSH/SFTPクライアント(`russh` + `russh-sftp`の
    /// クライアントAPI)が、実TCPループバック上でこのモジュールの
    /// `SshSession`/`SftpSubsystem`サーバーへ接続し、公開鍵認証→
    /// ディレクトリ作成→ファイルアップロード→一覧取得→ダウンロード→
    /// 削除まで一気通貫で成功することを実証する(型チェックのみでの
    /// 完了報告はしない、既存の運用ルールに従う)。
    #[tokio::test]
    async fn real_sftp_client_roundtrip_over_loopback() {
        let tmp_root = std::env::temp_dir().join(format!("owsftp-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_root).unwrap();
        let root = tmp_root.canonicalize().unwrap();

        // クライアント側の鍵ペアを生成し、サーバーの許可鍵リストへ直接
        // 登録する(authorized_keysファイルのパース自体は
        // `load_authorized_keys`の別テストで既に検証済みのため、ここでは
        // 認証成功後のSFTP往復そのものに焦点を当てる)。
        let client_keypair = KeyPair::generate_ed25519().expect("ed25519 keygen should succeed");
        let client_public_key = client_keypair.clone_public_key().expect("public key extraction should succeed");

        let host_keypair = KeyPair::generate_ed25519().expect("ed25519 keygen should succeed");
        let config = std::sync::Arc::new(russh::server::Config {
            auth_rejection_time: std::time::Duration::from_millis(50),
            keys: vec![host_keypair],
            ..Default::default()
        });

        let mut app_server = SftpAppServer {
            root: Arc::new(root.clone()),
            authorized_keys: Arc::new(vec![client_public_key]),
            allow_password: false,
            password: Arc::new(None),
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use russh::server::Server as _;
            let _ = app_server.run_on_socket(config, &listener).await;
        });

        // クライアント接続・公開鍵認証。
        let client_config = std::sync::Arc::new(russh::client::Config::default());
        let mut handle = russh::client::connect(client_config, addr, AcceptAnyServerKey)
            .await
            .expect("SSH client should connect to the loopback SFTP server");
        let authenticated = handle
            .authenticate_publickey("sftp-e2e-test-user", Arc::new(client_keypair))
            .await
            .expect("publickey auth request should not error");
        assert!(authenticated, "server should accept the authorized client public key");

        // SFTPサブシステムを開き、クライアントセッションを確立する。
        let channel = handle.channel_open_session().await.unwrap();
        channel.request_subsystem(true, "sftp").await.unwrap();
        let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .expect("SFTP protocol handshake should succeed");

        // ディレクトリ作成→アップロード→一覧取得→ダウンロード→削除、の往復。
        sftp.create_dir("uploads").await.expect("mkdir should succeed");

        let payload = b"open-web-server embedded SFTP roundtrip test payload".to_vec();
        sftp.write("uploads/roundtrip.txt", &payload).await.expect("upload (write) should succeed");

        let listing = sftp.read_dir("uploads").await.expect("readdir should succeed");
        let names: Vec<String> = listing.map(|entry| entry.file_name()).collect();
        assert!(names.contains(&"roundtrip.txt".to_string()), "uploaded file should be visible in directory listing: {names:?}");

        let downloaded = sftp.read("uploads/roundtrip.txt").await.expect("download (read) should succeed");
        assert_eq!(downloaded, payload, "downloaded content must match what was uploaded");

        sftp.remove_file("uploads/roundtrip.txt").await.expect("remove should succeed");
        assert!(!root.join("uploads/roundtrip.txt").exists(), "file must be actually gone from disk after remove");

        let _ = std::fs::remove_dir_all(&root);
    }
}
