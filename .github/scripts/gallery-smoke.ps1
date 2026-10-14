# Gallery を起動し、サイドバーの全ページを巡回して、同梱した Windows App SDK
# ランタイムだけで動いていることを確かめる。
#
# - ウィンドウが出るまで待ち、出る前に終われば標準エラー (naui のエラー) を出して失敗する
# - サイドバーの項目を UI オートメーションで 1 つずつ選び、落ちないことを見る
# - 読み込んだ Microsoft.UI.Xaml.dll が exe の隣のものであり、OS へ導入された
#   フレームワークパッケージ (WindowsApps) からは何も読んでいないことを見る
param(
    [Parameter(Mandatory)]
    [string] $Exe
)

$ErrorActionPreference = 'Stop'

$exe = (Resolve-Path $Exe).Path
$directory = Split-Path $exe
$stderr = Join-Path $env:RUNNER_TEMP 'gallery-stderr.txt'

$version = [Environment]::OSVersion.Version
Write-Host "OS: $version (Windows 11 24H2 = ビルド 26100 より前なら、Windows App SDK が reg-free WinRT を受け持つ)"

$process = Start-Process -FilePath $exe -WorkingDirectory $env:RUNNER_TEMP -PassThru `
    -RedirectStandardError $stderr

$deadline = (Get-Date).AddSeconds(60)
while ((Get-Date) -lt $deadline) {
    $process.Refresh()
    if ($process.HasExited -or $process.MainWindowHandle -ne [IntPtr]::Zero) { break }
    Start-Sleep -Milliseconds 500
}
if ($process.HasExited) {
    Get-Content $stderr -ErrorAction SilentlyContinue | Write-Host
    throw ("Gallery が起動直後に終了しました (終了コード 0x{0:X8})" -f $process.ExitCode)
}
if ($process.MainWindowHandle -eq [IntPtr]::Zero) {
    Stop-Process -Id $process.Id -Force
    throw 'Gallery のウィンドウが 60 秒たっても出ません'
}

try {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    $automation = [System.Windows.Automation.AutomationElement]
    $root = $automation::FromHandle($process.MainWindowHandle)
    $condition = New-Object System.Windows.Automation.PropertyCondition(
        $automation::ControlTypeProperty, [System.Windows.Automation.ControlType]::ListItem)
    $items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    if ($items.Count -eq 0) { throw 'サイドバーの項目が見つかりません' }

    foreach ($item in $items) {
        $name = $item.Current.Name
        $item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
        Start-Sleep -Milliseconds 1500
        $process.Refresh()
        if ($process.HasExited) {
            Get-Content $stderr -ErrorAction SilentlyContinue | Write-Host
            throw ("「{0}」を開いたところで落ちました (終了コード 0x{1:X8})" -f $name, $process.ExitCode)
        }
        Write-Host "ok: $name"
    }

    $modules = $process.Modules
    $fromPackages = @($modules | Where-Object { $_.FileName -like '*\WindowsApps\*' })
    $xaml = $modules | Where-Object { $_.ModuleName -eq 'Microsoft.UI.Xaml.dll' } | Select-Object -First 1
    Write-Host "Microsoft.UI.Xaml.dll: $($xaml.FileName)"
    if ($fromPackages.Count -gt 0) {
        $fromPackages | ForEach-Object { Write-Host "  $($_.FileName)" }
        throw "フレームワークパッケージから $($fromPackages.Count) 個のモジュールを読んでいます"
    }
    if (-not $xaml -or -not $xaml.FileName.StartsWith($directory, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Microsoft.UI.Xaml.dll が exe の隣から読まれていません'
    }
    Write-Host "$($items.Count) ページを巡回し、すべて同梱したランタイムで動きました"
}
finally {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
}
