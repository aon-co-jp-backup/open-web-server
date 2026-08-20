; open-web-server-install.exe — Inno Setup script (2026-08-19新設)
;
; ユーザー指示「{リポジトリ名}-install.exe用のInno Setup .issファイルを
; 作成する」への対応。open-easy-web側と同じ設計方針(既存install.ps1/
; uninstall.ps1をラップするだけ、ロジック二重実装を避ける)。
;
; ビルド方法: この開発機にInno Setup(ISCC.exe)は導入されていないため
; 未ビルド(正直な開示)。事前に `cargo build --release --features
; acme,ddns,sftp,upnp` 等で open-web-server.exe を生成し、本ファイルと
; 同じ installer\ ディレクトリへ配置してから `iscc
; open-web-server-install.iss` を実行すること。

#define MyAppName "open-web-server"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "aon-co-jp"
#define MyAppURL "https://github.com/aon-co-jp/open-web-server"
#define MyServiceName "OpenWebServer"

[Setup]
AppId={{A3D1F2E4-1A2B-4C3D-8E9F-OPENWEBSERVER}}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
DefaultDirName={autopf}\open-web-server
DefaultGroupName=open-web-server
DisableProgramGroupPage=yes
OutputDir=.
OutputBaseFilename=open-web-server-install
Compression=lzma2
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
UninstallDisplayIcon={app}\open-web-server.exe

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "open-web-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\install.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\uninstall.ps1"; DestDir: "{app}"; Flags: ignoreversion
; 自己アップデート機能(self_update.rs、auto-update feature)が起動ファイル
; 隣の version.json を参照するため同梱する(無ければ「未インストール扱い」
; として自己アップデートが無効化される、既存の安全設計)。
Source: "version.json"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Run]
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\install.ps1"""; \
    WorkingDir: "{app}"; StatusMsg: "open-web-server サービスをセットアップしています..."; \
    Flags: runhidden waituntilterminated

[UninstallRun]
Filename: "powershell.exe"; \
    Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\uninstall.ps1"""; \
    WorkingDir: "{app}"; RunOnceId: "UninstallOpenWebServer"; Flags: runhidden waituntilterminated
