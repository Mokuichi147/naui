; Gallery のインストーラー .exe (Inno Setup 6)。build-installers.ps1 から作る。
;
;   Source: インストールする中身を置いたディレクトリ
;   AppVersion: 版 (0.9.1 など)
;   LicenseFile: 使用許諾の画面に出すテキスト (UTF-8、BOM 付き)
;
; AppId は変えない。変えると、古い版を残したまま別のアプリとして入る。
; gallery.wxs が AppId からこのインストーラーの登録を探すので、変えるならそちらも直す。

#ifndef Source
  #error Source を /DSource=... で渡してください
#endif

[Setup]
AppId={{8BDF724B-55BB-405D-93F3-07ABA26C558B}
AppName=naui UI gallery
AppVersion={#AppVersion}
AppPublisher=naui
DefaultDirName={autopf}\naui UI gallery
DisableProgramGroupPage=yes
; 既定は自分だけに入れる (管理者権限が要らない)。全員に入れるかは起動時に選べる。
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; 同梱したランタイムが動く下限 (Windows 10 1903)。
MinVersion=10.0.18362
LicenseFile={#LicenseFile}
UninstallDisplayIcon={app}\gallery.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; ランタイムの DLL は exe の隣に無いと読み込まれない。ディレクトリごと入れる。
Source: "{#Source}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\naui UI gallery"; Filename: "{app}\gallery.exe"
Name: "{autodesktop}\naui UI gallery"; Filename: "{app}\gallery.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\gallery.exe"; Description: "{cm:LaunchProgram,naui UI gallery}"; Flags: nowait postinstall skipifsilent

[CustomMessages]
japanese.MsiInstalled=naui UI gallery の .msi 版がインストールされています。%n%n.msi 版と同じフォルダーとショートカットを使うため、重ねてインストールできません。先に「設定」の「アプリ」から naui UI gallery をアンインストールしてください。
english.MsiInstalled=The .msi version of naui UI gallery is already installed.%n%nIt uses the same folder and shortcut, so this installer cannot be installed alongside it. Uninstall naui UI gallery from Settings > Apps first.

[Code]
// .msi 版 (gallery.wxs) と同じフォルダーとショートカットを使う。両方を入れると、
// 片方のアンインストールがもう片方のファイルを消すので、.msi 版があれば止める。
// gallery.wxs の UpgradeCode を変えるなら、ここも直す。
const
  MsiUpgradeCode = '{8C1EB857-60C1-4077-BA40-D983CD6504D4}';
  ERROR_SUCCESS = 0;

function MsiEnumRelatedProducts(UpgradeCode: String; Reserved: DWORD; Index: DWORD; ProductBuf: String): Cardinal;
  external 'MsiEnumRelatedProductsW@msi.dll stdcall';

function InitializeSetup(): Boolean;
var
  Product: String;
begin
  // ProductCode ({...} の 38 文字) と終端の 0 が入る大きさ。
  Product := StringOfChar(' ', 39);
  Result := MsiEnumRelatedProducts(MsiUpgradeCode, 0, 0, Product) <> ERROR_SUCCESS;
  if not Result then
    SuppressibleMsgBox(CustomMessage('MsiInstalled'), mbCriticalError, MB_OK, IDOK);
end;
