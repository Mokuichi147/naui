# Gallery を起動し、サイドバーの全ページを巡回して、同梱した Windows App SDK
# ランタイムだけで動いていることを確かめる。
#
# - ウィンドウが出るまで待ち、出る前に終われば標準エラー (naui のエラー) を出して失敗する
# - サイドバーの項目を UI オートメーションで 1 つずつ選び、落ちないことを見る
# - 読み込んだ Microsoft.UI.Xaml.dll が exe の隣のものであり、OS へ導入された
#   フレームワークパッケージ (WindowsApps) からは何も読んでいないことを見る
#
# 失敗したときは、プロセスのウィンドウ・UI オートメーションの木・読み込んだ
# モジュールをログへ出し、画面のスクリーンショットを `<RUNNER_TEMP>/<Label>.png`
# に残す。
param(
    [Parameter(Mandatory)]
    [string] $Exe,
    # スクリーンショットの名前。同じジョブで何度か走らせるときに分ける。
    [string] $Label = 'gallery-smoke'
)

$ErrorActionPreference = 'Stop'

$exe = (Resolve-Path $Exe).Path
$directory = Split-Path $exe
$stderr = Join-Path $env:RUNNER_TEMP "$Label-stderr.txt"

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class SmokeWindows {
    delegate bool EnumProc(IntPtr hwnd, IntPtr lparam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc, IntPtr lparam);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder text, int max);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
    public static List<string> Describe(uint processId) {
        var found = new List<string>();
        EnumWindows((hwnd, _) => {
            uint pid;
            GetWindowThreadProcessId(hwnd, out pid);
            if (pid == processId) {
                var cls = new StringBuilder(256); GetClassName(hwnd, cls, 256);
                var title = new StringBuilder(256); GetWindowText(hwnd, title, 256);
                found.Add(string.Format("0x{0:X} visible={1} class={2} title={3}", hwnd.ToInt64(), IsWindowVisible(hwnd), cls, title));
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

function Write-Diagnostics($process, $root) {
    Write-Host '--- 診断 ---'
    try {
        Write-Host "MainWindowHandle: 0x$('{0:X}' -f $process.MainWindowHandle.ToInt64()) title=$($process.MainWindowTitle)"
        Write-Host 'プロセスのトップレベルウィンドウ:'
        [SmokeWindows]::Describe([uint32] $process.Id) | ForEach-Object { Write-Host "  $_" }
        if ($root) {
            Write-Host 'UI オートメーションの木 (Raw ビュー、深さ 8 まで、先頭 120 要素):'
            $walker = [System.Windows.Automation.TreeWalker]::RawViewWalker
            $queue = New-Object System.Collections.Queue
            $queue.Enqueue(@($root, 0))
            $shown = 0
            while ($queue.Count -gt 0 -and $shown -lt 120) {
                $entry = $queue.Dequeue()
                $element = $entry[0]
                $depth = $entry[1]
                $current = $element.Current
                Write-Host ('  {0}{1} "{2}" class={3}' -f ('  ' * $depth),
                    $current.ControlType.ProgrammaticName, $current.Name, $current.ClassName)
                $shown++
                if ($depth -ge 8) { continue }
                $child = $walker.GetFirstChild($element)
                while ($child) {
                    $queue.Enqueue(@($child, ($depth + 1)))
                    $child = $walker.GetNextSibling($child)
                }
            }
        }
        if (-not $process.HasExited) {
            Write-Host '読み込んだ WinUI 関連のモジュール:'
            $process.Modules |
                Where-Object { $_.ModuleName -match '^(Microsoft\.|WinUIEdit|CoreMessagingXP|dcompi|DWriteCore)' } |
                ForEach-Object { Write-Host "  $($_.FileName)" }
        }
    }
    catch {
        Write-Host "診断の途中で失敗しました: $_"
    }
    try {
        Add-Type -AssemblyName System.Drawing, System.Windows.Forms
        $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
        $path = Join-Path $env:RUNNER_TEMP "$Label.png"
        $bitmap.Save($path)
        Write-Host "スクリーンショット: $path"
    }
    catch {
        Write-Host "スクリーンショットを撮れませんでした: $_"
    }
    Get-Content $stderr -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "stderr: $_" }
}

$version = [Environment]::OSVersion.Version
Write-Host "OS: $version (Windows 11 24H2 = ビルド 26100 より前なら、Windows App SDK が reg-free WinRT を受け持つ)"

$process = Start-Process -FilePath $exe -WorkingDirectory $env:RUNNER_TEMP -PassThru `
    -RedirectStandardError $stderr
$root = $null

try {
    $deadline = (Get-Date).AddSeconds(60)
    while ((Get-Date) -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited -or $process.MainWindowHandle -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 500
    }
    if ($process.HasExited) {
        throw ("Gallery が起動直後に終了しました (終了コード 0x{0:X8})" -f $process.ExitCode)
    }
    if ($process.MainWindowHandle -eq [IntPtr]::Zero) {
        throw 'Gallery のウィンドウが 60 秒たっても出ません'
    }

    $automation = [System.Windows.Automation.AutomationElement]
    $root = $automation::FromHandle($process.MainWindowHandle)
    $condition = New-Object System.Windows.Automation.PropertyCondition(
        $automation::ControlTypeProperty, [System.Windows.Automation.ControlType]::ListItem)
    # ウィンドウは中身より先に出るので、サイドバーの項目が現れるまで待つ。
    $deadline = (Get-Date).AddSeconds(60)
    do {
        $items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
        if ($items.Count -gt 0) { break }
        $process.Refresh()
        if ($process.HasExited) {
            throw ("中身が出る前に終了しました (終了コード 0x{0:X8})" -f $process.ExitCode)
        }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    if ($items.Count -eq 0) { throw 'サイドバーの項目が 60 秒たっても見つかりません' }

    foreach ($item in $items) {
        $name = $item.Current.Name
        $item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
        Start-Sleep -Milliseconds 1500
        $process.Refresh()
        if ($process.HasExited) {
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
catch {
    Write-Diagnostics $process $root
    throw
}
finally {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
}
