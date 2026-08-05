import COpenWebServerBridge
import Foundation

/// `open-web-server`本体(Rust)をこのアプリのプロセス内で起動・監視する
/// Swift側の薄いラッパー。実体は`crates/open-web-server-ios-bridge`の
/// C ABI関数(`owic_*`)を呼ぶだけ(Android版`MainActivity.
/// startServerProcess()`の`ProcessBuilder`起動に相当するが、iOSは
/// 任意バイナリのサブプロセス起動を許可しないため、代わりに**同一
/// プロセス内でRustライブラリ関数を直接呼ぶ**設計になっている点が
/// Android版との本質的な違い)。
///
/// # 正直な開示・iOSの制約
/// - フォアグラウンドにある間しか、他端末からの接続を安定して受け付け
///   られない(iOSのバックグラウンド実行制限、`PowerProfile`のdoc参照)。
/// - `owic_stop()`は現状未実装(常に`false`)。一度`start()`したサーバーを
///   このプロセス内で明示的に止める手段は無い——アプリごと終了させるまで
///   動き続ける(Android版のような「プロセスをkillして再起動」に相当する
///   操作はiOSアプリの通常のライフサイクル外であり、意図的に実装して
///   いない)。
@MainActor
public final class ServerBridge: ObservableObject {
    public enum Status: Equatable {
        case notStarted
        case starting
        /// `GET /healthz`が実際に200を返すまで確認できたら`.healthy`。
        case healthy
        /// 起動要求はしたが、まだ`/healthz`から200を得られていない。
        case waitingForHealthCheck
    }

    @Published public private(set) var status: Status = .notStarted
    @Published public private(set) var lastHealthCheckAt: Date?

    private var pollTask: Task<Void, Never>?

    public init() {}

    /// `bindHost:bindPort`で起動する。`profile`はポーリング間隔にのみ
    /// 影響する(`PowerProfile.healthPollInterval`参照)——サーバー本体の
    /// 挙動そのものを変えるには、既存の`POST /admin/power-profile`
    /// 管理APIを別途呼ぶこと(Windows/Linux/Android版と同じAPI)。
    public func start(bindHost: String = "127.0.0.1", bindPort: UInt16 = 18099, profile: PowerProfile = .normal) {
        guard status == .notStarted else { return }
        status = .starting

        setEnv("OPEN_WEB_SERVER_BIND", "\(bindHost):\(bindPort)")

        let started = owic_start()
        guard started else {
            status = .notStarted
            return
        }

        status = .waitingForHealthCheck
        beginPolling(host: bindHost, port: bindPort, profile: profile)
    }

    private func setEnv(_ key: String, _ value: String) {
        key.withCString { keyPtr in
            value.withCString { valuePtr in
                _ = owic_set_env(keyPtr, valuePtr)
            }
        }
    }

    private func beginPolling(host: String, port: UInt16, profile: PowerProfile) {
        pollTask?.cancel()
        pollTask = Task { [weak self] in
            guard let self else { return }
            let url = URL(string: "http://\(host):\(port)/healthz")!
            while !Task.isCancelled {
                if let (_, response) = try? await URLSession.shared.data(from: url),
                   let http = response as? HTTPURLResponse,
                   http.statusCode == 200 {
                    await MainActor.run {
                        self.status = .healthy
                        self.lastHealthCheckAt = Date()
                    }
                }
                try? await Task.sleep(nanoseconds: UInt64(profile.healthPollInterval * 1_000_000_000))
            }
        }
    }

    deinit {
        pollTask?.cancel()
    }
}
