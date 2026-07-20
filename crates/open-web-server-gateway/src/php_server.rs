//! PHP実行(PHPビルトインdevサーバをサブプロセスとして起動し、リバース
//! プロキシで中継する第一実装)。
//!
//! 本来の本番運用ではPHP-FPM + FastCGIが定番だが、`php-fpm`はこの開発
//! 環境(Windows + WinGet配布のPHP)には同梱されておらず、`php.exe -S`
//! (PHPのビルトイン開発用Webサーバ)は標準で使えることを確認済み。
//! Apache+Nginxハイブリッド配信エンジンとしての「PHP実行」を最初の1歩として
//! 現実的に検証可能にするため、まずは`php -S 127.0.0.1:<port> -t <docroot>`
//! をサブプロセス起動し、HTTPリバースプロキシで中継する設計を採用した
//! (`proxy::forward_to`と同じ転送ロジックを再利用する)。本番向けの
//! PHP-FPM/FastCGI直結は将来の拡張候補として明記する。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// docrootごとに起動された`php -S`子プロセスの実行時ハンドル。
struct PhpInstance {
    _child: Child,
    addr: String,
}

/// PHPビルトインサーバのサブプロセスをdocrootごとに遅延起動・使い回す
/// プール。同じdocrootに対して複数回リクエストが来ても子プロセスは
/// 1つだけ起動する(初回起動時のみ待ち合わせが発生する)。
pub struct PhpServerPool {
    php_binary: PathBuf,
    instances: Mutex<HashMap<PathBuf, Arc<PhpInstance>>>,
    next_port: AtomicU16,
}

impl PhpServerPool {
    /// `php_binary`: `php`実行ファイルへのパス(configurable、環境変数
    /// `OPEN_WEB_SERVER_PHP_BINARY`が優先、未設定時はこのユーザー環境に
    /// 実際にインストールされているWinGet配布パスをデフォルトとする)。
    pub fn new(php_binary: PathBuf) -> Self {
        Self {
            php_binary,
            instances: Mutex::new(HashMap::new()),
            // 8000番台の空きポートを順に割り当てる(単純な採番、衝突時は
            // OS側のbindエラーで検出されるが、本プロセスの生存期間中は
            // 同じdocrootに対して1回しか起動しないため実用上問題ない)。
            next_port: AtomicU16::new(8091),
        }
    }

    pub fn from_env() -> Self {
        let binary = std::env::var("OPEN_WEB_SERVER_PHP_BINARY").unwrap_or_else(|_| {
            r"C:\Users\noruk\AppData\Local\Microsoft\WinGet\Packages\PHP.PHP.8.3_Microsoft.Winget.Source_8wekyb3d8bbwe\php.exe".to_string()
        });
        Self::new(PathBuf::from(binary))
    }

    /// `docroot`向けの`php -S`インスタンスを取得する(未起動なら起動する)。
    /// 戻り値は`"127.0.0.1:<port>"`形式のアドレス文字列で、
    /// `proxy::forward_to`にそのまま渡せる。
    pub async fn ensure_running(&self, docroot: &Path) -> anyhow::Result<String> {
        let mut guard = self.instances.lock().await;
        if let Some(instance) = guard.get(docroot) {
            return Ok(instance.addr.clone());
        }

        let port = self.next_port.fetch_add(1, Ordering::SeqCst);
        let addr = format!("127.0.0.1:{port}");

        let child = Command::new(&self.php_binary)
            .arg("-S")
            .arg(&addr)
            .arg("-t")
            .arg(docroot)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to spawn php built-in server ({}): {e}",
                    self.php_binary.display()
                )
            })?;

        // php -S はリスンソケットの確立に多少のリードタイムがあるため、
        // 最初の接続を試みる前に短時間待つ(起動直後の接続refusedを避ける)。
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        guard.insert(
            docroot.to_path_buf(),
            Arc::new(PhpInstance {
                _child: child,
                addr: addr.clone(),
            }),
        );

        Ok(addr)
    }

    /// テスト/シャットダウン用: 現在起動中のインスタンス数。
    pub async fn running_count(&self) -> usize {
        self.instances.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn php_available() -> Option<PathBuf> {
        let path = PathBuf::from(
            std::env::var("OPEN_WEB_SERVER_PHP_BINARY").unwrap_or_else(|_| {
                r"C:\Users\noruk\AppData\Local\Microsoft\WinGet\Packages\PHP.PHP.8.3_Microsoft.Winget.Source_8wekyb3d8bbwe\php.exe".to_string()
            }),
        );
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    #[tokio::test]
    async fn ensure_running_reuses_instance_for_same_docroot() {
        let Some(php) = php_available() else {
            eprintln!("skipping: php binary not present in this environment");
            return;
        };

        let dir = std::env::temp_dir().join(format!("owstest-php-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.php"), b"<?php echo 'ok'; ?>").unwrap();

        let pool = PhpServerPool::new(php);
        let addr1 = pool.ensure_running(&dir).await.unwrap();
        let addr2 = pool.ensure_running(&dir).await.unwrap();
        assert_eq!(addr1, addr2, "same docroot should reuse the same php instance");
        assert_eq!(pool.running_count().await, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
