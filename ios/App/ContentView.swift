import OpenWebServerKit
import SwiftUI

struct ContentView: View {
    @StateObject private var bridge = ServerBridge()
    @State private var selectedProfile: PowerProfile = .normal

    var body: some View {
        NavigationStack {
            Form {
                Section("Power profile (電源プロファイル)") {
                    Picker("Profile", selection: $selectedProfile) {
                        ForEach(PowerProfile.allCases, id: \.self) { profile in
                            Text(profile.label).tag(profile)
                        }
                    }
                    .pickerStyle(.inline)
                }

                Section("Status (状態)") {
                    LabeledContent("Status", value: statusText)
                    if let lastCheck = bridge.lastHealthCheckAt {
                        LabeledContent("Last healthz OK", value: lastCheck.formatted(date: .omitted, time: .standard))
                    }
                    Button("Start server (サーバーを起動)") {
                        bridge.start(profile: selectedProfile)
                    }
                    .disabled(bridge.status != .notStarted)
                }

                Section("Honest disclosure (正直な開示)") {
                    Text(
                        """
                        This app only accepts incoming connections while it is in \
                        the foreground — iOS suspends background apps, so this is \
                        not an always-on server the way the Android/Windows/Linux \
                        versions are. / このアプリはフォアグラウンドにある間しか \
                        外部からの接続を受け付けられません(iOSのバックグラウンド \
                        実行制限のため、Android/Windows/Linux版のような常時稼働 \
                        サーバーではありません)。
                        """
                    )
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("open-web-server")
        }
    }

    private var statusText: String {
        switch bridge.status {
        case .notStarted: return "Not started (未起動)"
        case .starting: return "Starting… (起動中…)"
        case .waitingForHealthCheck: return "Waiting for /healthz… (応答待ち…)"
        case .healthy: return "Healthy ✅ (正常稼働)"
        }
    }
}

#Preview {
    ContentView()
}
