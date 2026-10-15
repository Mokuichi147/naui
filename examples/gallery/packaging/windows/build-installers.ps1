# Gallery の .msi (WiX v5) と、インストーラーの .exe (Inno Setup 6) を作る。
#
#   ./build-installers.ps1 -Source <中身のディレクトリ> -Version <版> -OutDir <出力先> -Name <ファイル名>
#
# Source には gallery.exe と、exe の隣に置く Windows App SDK ランタイム
# (target/release/naui-winappsdk/ の中身)、assets/、ライセンスを置いておく。
# OutDir に <Name>.msi と <Name>-setup.exe ができる。
#
# 使う道具 (CI では release.yml が入れる):
#   dotnet tool install --global wix --version 5.0.2
#   wix extension add --global WixToolset.UI.wixext/5.0.2
#   Inno Setup 6 (ISCC.exe)
param(
    [Parameter(Mandatory)] [string] $Source,
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $OutDir,
    [Parameter(Mandatory)] [string] $Name
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$here = $PSScriptRoot
$Source = (Resolve-Path $Source).Path
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$work = Join-Path ([System.IO.Path]::GetTempPath()) "naui-installers-$([guid]::NewGuid())"
New-Item -ItemType Directory $work | Out-Null

try {
    if (-not (Test-Path (Join-Path $Source 'gallery.exe'))) {
        throw "$Source に gallery.exe がありません。"
    }

    # MSI の版は数字と点だけ。0.9.1-rc1 の -rc1 は落とす。
    # 0.9.1-rc1 と 0.9.1 は同じ版になるので、gallery.wxs の MajorUpgrade で
    # 同じ版どうしも置き換える。
    $numeric = ($Version -split '-')[0]

    # 使用許諾の文面。WiX は RTF、Inno Setup は BOM 付きの UTF-8 を読む。
    $license = Get-Content -Raw -Encoding UTF8 (Join-Path $here 'license.txt')
    $licenseTxt = Join-Path $work 'license.txt'
    [System.IO.File]::WriteAllText($licenseTxt, $license, [System.Text.UTF8Encoding]::new($true))

    $rtf = [System.Text.StringBuilder]::new()
    [void]$rtf.Append('{\rtf1\ansi\deff0{\fonttbl{\f0\fnil Yu Gothic UI;}}\fs18 ')
    foreach ($ch in $license.ToCharArray()) {
        $code = [int]$ch
        if ($ch -eq "`r") { continue }
        if ($ch -eq "`n") { [void]$rtf.Append('\par ') }
        elseif ('\{}'.Contains($ch)) { [void]$rtf.Append('\').Append($ch) }
        elseif ($code -gt 127) {
            # RTF の \u は符号付き 16 ビット。
            if ($code -gt 32767) { $code -= 65536 }
            # "$code?" と書くと ? まで変数名になる。
            [void]$rtf.Append("\u${code}?")
        }
        else { [void]$rtf.Append($ch) }
    }
    [void]$rtf.Append('}')
    $licenseRtf = Join-Path $work 'license.rtf'
    [System.IO.File]::WriteAllText($licenseRtf, $rtf.ToString(), [System.Text.Encoding]::ASCII)

    # .msi
    $msi = Join-Path $OutDir "$Name.msi"
    & wix build (Join-Path $here 'gallery.wxs') `
        -arch x64 `
        -culture ja-JP `
        -ext WixToolset.UI.wixext `
        -d "Source=$Source" `
        -d "Version=$numeric" `
        -d "LicenseRtf=$licenseRtf" `
        -intermediatefolder (Join-Path $work 'wix') `
        -o $msi
    if ($LASTEXITCODE -ne 0) { throw "wix build が失敗しました ($LASTEXITCODE)。" }
    # wix build は .wixpdb も並べるので、配布しないものは消す。
    Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $OutDir "$Name.wixpdb")

    # インストーラーの .exe
    $iscc = Get-Command ISCC.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
    if (-not $iscc) {
        $iscc = Get-ChildItem 'C:\Program Files*\Inno Setup 6\ISCC.exe' -ErrorAction SilentlyContinue |
            Select-Object -First 1 -ExpandProperty FullName
    }
    if (-not $iscc) { throw 'Inno Setup 6 (ISCC.exe) が見つかりません。' }
    & $iscc /Q `
        "/DSource=$Source" `
        "/DAppVersion=$Version" `
        "/DLicenseFile=$licenseTxt" `
        "/O$OutDir" `
        "/F$Name-setup" `
        (Join-Path $here 'gallery.iss')
    if ($LASTEXITCODE -ne 0) { throw "ISCC が失敗しました ($LASTEXITCODE)。" }
}
finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $work
}
