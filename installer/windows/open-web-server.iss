; open-web-server Windowsインストーラー(Inno Setup)。
;
; ユーザー指示「open-english/installer/windows/open-english.iss を
; モデルにopen-web-server用の.issを作成する」への対応。実際の
; セットアップロジック(Windowsサービス`OpenWebServer`の登録・削除)は
; 二重実装を避けるため既存の`install.ps1`/`uninstall.ps1`(リポジトリ
; ルート)をそのまま呼ぶだけに留める——この設計は
; `installer/open-web-server-install.iss`(同日、別セッションが先に
; 作成)と同じ方針であり、本ファイルはユーザー指定のパス
; (`installer/windows/open-web-server.iss`)に合わせて配置した版。
;
; ビルド方法: 事前に`cargo build --release --features
; acme,ddns,sftp,upnp,auto-update`等でopen-web-server.exeを生成し、
; このディレクトリ(installer\windows\)へ配置してから
; `ISCC.exe open-web-server.iss`を実行する。

#define MyAppName "open-web-server"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#define MyAppPublisher "aon-co-jp"
#define MyAppURL "https://github.com/aon-co-jp/open-web-server"
#define MyAppExeName "open-web-server.exe"
#define MyServiceName "OpenWebServer"

[Setup]
; Windowsサービス登録(`New-Service`)は管理者権限を要するため、
; open-englishの`PrivilegesRequired=lowest`とは異なりここは`admin`のまま
; とする(`install.ps1`冒頭の`#Requires -RunAsAdministrator`と整合)。
PrivilegesRequired=admin
AppId={{A3D1F2E4-1A2B-4C3D-8E9F-0PENWEBSERVER1}}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes
OutputDir=.
; open-englishと同じ命名規則(`<アプリ名>-install.exe`、バージョン番号
; なしの固定ファイル名——`self_update.rs`のWindows向けアセット判定は
; zip配布物〈`open-web-server-windows-x86_64.zip`〉を対象にしており
; インストーラーファイル名自体は関与しないが、利用者が一目でインストーラー
; と分かる名前にするという既存エコシステムの命名慣習を踏襲する)。
OutputBaseFilename=open-web-server-install
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
; ビルド済みバイナリはこのディレクトリ(installer\windows\)へ事前に
; コピーしておく必要がある(open-englishの`..\..\server\target\release\`
; 参照とは異なり、このリポジトリのCI成果物レイアウトに合わせて
; インストーラーと同じディレクトリからの取得とした)。
Source: "open-web-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\install.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\uninstall.ps1"; DestDir: "{app}"; Flags: ignoreversion
; 自己アップデート機能(`self_update.rs`、`auto-update` feature)が
; 実行ファイル隣の`version.json`の有無で「インストール済み配布物か」を
; 判定するため同梱する(無ければ自己アップデートは常に無効、既存の
; 安全設計——open-english側と同じ判断)。
Source: "version.json"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Run]
; 実際のサービス登録ロジックは`install.ps1`に委譲する(二重実装を避ける、
; `installer/open-web-server-install.iss`と同じ設計)。
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\install.ps1"""; \
    WorkingDir: "{app}"; StatusMsg: "open-web-server サービスをセットアップしています... / Setting up the open-web-server service..."; \
    Flags: runhidden waituntilterminated

[UninstallRun]
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\uninstall.ps1"""; \
    WorkingDir: "{app}"; RunOnceId: "UninstallOpenWebServer"; Flags: runhidden waituntilterminated

[UninstallDelete]
Type: filesandordirs; Name: "{app}"
