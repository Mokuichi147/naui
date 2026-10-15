; Gallery のインストーラー .exe (Inno Setup 6)。build-installers.ps1 から作る。
;
;   Source: インストールする中身を置いたディレクトリ
;   AppVersion: 版 (0.9.1 など)
;   LicenseFile: 使用許諾の画面に出すテキスト (UTF-8、BOM 付き)
;
; AppId は変えない。変えると、古い版を残したまま別のアプリとして入る。

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
