[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$Executable,
    [ValidateRange(5, 120)]
    [int]$TimeoutSeconds = 30
)

$ErrorActionPreference = "Stop"
if ($env:OS -ne "Windows_NT") {
    throw "This runtime verification requires Windows."
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class YeetNativeWindow
{
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr parameter);

    [StructLayout(LayoutKind.Sequential)]
    public struct Rect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowTextLengthW(IntPtr hwnd);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowTextW(IntPtr hwnd, StringBuilder text, int maximum);

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
    public static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern int GetSystemMetrics(int index);
}
'@

$GwlExStyle = -20
$WsExTopmost = 0x00000008L
$WsExToolWindow = 0x00000080L
$WsExNoActivate = 0x08000000L
$SmMonitors = 80

function Get-ProcessWindows([uint32]$ProcessId) {
    $windows = [Collections.Generic.List[object]]::new()
    $callback = [YeetNativeWindow+EnumWindowsProc] {
        param([IntPtr]$Handle, [IntPtr]$Parameter)

        [uint32]$owner = 0
        [void][YeetNativeWindow]::GetWindowThreadProcessId($Handle, [ref]$owner)
        if ($owner -eq $ProcessId) {
            $length = [YeetNativeWindow]::GetWindowTextLengthW($Handle)
            $title = [Text.StringBuilder]::new($length + 1)
            [void][YeetNativeWindow]::GetWindowTextW($Handle, $title, $title.Capacity)
            $rect = [YeetNativeWindow+Rect]::new()
            [void][YeetNativeWindow]::GetWindowRect($Handle, [ref]$rect)
            $windows.Add([pscustomobject]@{
                Handle = $Handle
                Title = $title.ToString()
                ExStyle = [YeetNativeWindow]::GetWindowLongPtr($Handle, $GwlExStyle).ToInt64()
                Visible = [YeetNativeWindow]::IsWindowVisible($Handle)
                Left = $rect.Left
                Top = $rect.Top
                Width = $rect.Right - $rect.Left
                Height = $rect.Bottom - $rect.Top
            })
        }
        return $true
    }
    if (-not [YeetNativeWindow]::EnumWindows($callback, [IntPtr]::Zero)) {
        throw "EnumWindows failed."
    }
    return @($windows)
}

function Assert-Style([object]$Window, [long]$Style, [string]$Name) {
    if (($Window.ExStyle -band $Style) -ne $Style) {
        throw "'$($Window.Title)' is missing $Name (extended style 0x$('{0:X8}' -f $Window.ExStyle))."
    }
}

function Test-Style([object]$Window, [long]$Style) {
    return ($Window.ExStyle -band $Style) -eq $Style
}

function Invoke-Toggle([string]$Path) {
    $toggle = Start-Process -FilePath $Path -ArgumentList "--toggle" -Wait -PassThru
    if ($toggle.ExitCode -ne 0) {
        throw "Forwarded --toggle exited with code $($toggle.ExitCode)."
    }
}

function Wait-ShelfVisibility([uint32]$ProcessId, [bool]$Visible, [DateTime]$Deadline) {
    do {
        Start-Sleep -Milliseconds 200
        $candidate = @(
            Get-ProcessWindows -ProcessId $ProcessId |
                Where-Object Title -eq "Yeet" |
                Select-Object -First 1
        )
        if ($candidate.Count -eq 1 -and $candidate[0].Visible -eq $Visible) {
            return $candidate[0]
        }
    } while ([DateTime]::UtcNow -lt $Deadline)
    throw "The Yeet shelf did not become visible=$Visible before the timeout."
}

$executablePath = (Resolve-Path -LiteralPath $Executable).Path
$testRoot = Join-Path ([IO.Path]::GetTempPath()) "yeet-runtime-$([Guid]::NewGuid().ToString('N'))"
$profileRoot = Join-Path $testRoot "profile"
$sampleFile = Join-Path $testRoot "runtime check.txt"
$process = $null

New-Item -ItemType Directory -Path $profileRoot -Force | Out-Null
Set-Content -LiteralPath $sampleFile -Value "Yeet Windows runtime verification" -Encoding utf8NoBOM

# Keep the test deterministic and avoid touching the runner account's real
# Yeet settings or persisted shelf.
$env:APPDATA = Join-Path $profileRoot "Roaming"
$env:LOCALAPPDATA = Join-Path $profileRoot "Local"
New-Item -ItemType Directory -Path $env:APPDATA, $env:LOCALAPPDATA -Force | Out-Null
$env:GSK_RENDERER = "cairo"

try {
    $quotedSample = '"' + $sampleFile.Replace('"', '\"') + '"'
    $process = Start-Process -FilePath $executablePath -ArgumentList $quotedSample -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $shelf = $null
    $edges = @()
    $monitorCount = [YeetNativeWindow]::GetSystemMetrics($SmMonitors)
    if ($monitorCount -lt 1) {
        throw "Windows reported no display monitors."
    }

    do {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) {
            throw "yeet.exe exited early with code $($process.ExitCode)."
        }
        $windows = Get-ProcessWindows -ProcessId $process.Id
        $shelf = @($windows | Where-Object Title -eq "Yeet" | Select-Object -First 1)
        $edges = @($windows | Where-Object Title -eq "Yeet edge")
        $shelfReady = $shelf.Count -eq 1 -and
            $shelf[0].Visible -and
            (Test-Style $shelf[0] $WsExTopmost) -and
            (Test-Style $shelf[0] $WsExToolWindow)
        $edgesReady = $edges.Count -eq $monitorCount
        if ($edgesReady) {
            foreach ($edge in $edges) {
                $edgesReady = $edgesReady -and
                    $edge.Visible -and
                    (Test-Style $edge $WsExTopmost) -and
                    (Test-Style $edge $WsExToolWindow) -and
                    (Test-Style $edge $WsExNoActivate)
            }
        }
    } while ((-not $shelfReady -or -not $edgesReady) -and [DateTime]::UtcNow -lt $deadline)

    if ($shelf.Count -ne 1) {
        throw "Expected one visible Yeet shelf HWND; found $($shelf.Count)."
    }
    $shelf = $shelf[0]
    if (-not $shelf.Visible) {
        throw "The Yeet shelf HWND exists but is not visible after adding a file."
    }
    if ($shelf.Width -lt 200 -or $shelf.Height -lt 200) {
        throw "The Yeet shelf has an invalid size: $($shelf.Width)x$($shelf.Height)."
    }
    Assert-Style $shelf $WsExTopmost "WS_EX_TOPMOST"
    Assert-Style $shelf $WsExToolWindow "WS_EX_TOOLWINDOW"

    if ($edges.Count -ne $monitorCount) {
        throw "Expected one edge HWND per monitor ($monitorCount); found $($edges.Count)."
    }
    foreach ($edge in $edges) {
        if (-not $edge.Visible) {
            throw "A Yeet edge HWND exists but is not visible."
        }
        if ($edge.Width -lt 3 -or $edge.Width -gt 64 -or $edge.Height -lt 200) {
            throw "A Yeet edge has an invalid size: $($edge.Width)x$($edge.Height)."
        }
        Assert-Style $edge $WsExTopmost "WS_EX_TOPMOST"
        Assert-Style $edge $WsExToolWindow "WS_EX_TOOLWINDOW"
        Assert-Style $edge $WsExNoActivate "WS_EX_NOACTIVATE"
    }

    # Exercise single-instance command forwarding and the map callback that
    # reapplies HWND_TOPMOST after the shelf has been hidden.
    Invoke-Toggle $executablePath
    [void](Wait-ShelfVisibility -ProcessId $process.Id -Visible $false `
        -Deadline ([DateTime]::UtcNow.AddSeconds($TimeoutSeconds)))
    Invoke-Toggle $executablePath
    $remappedShelf = Wait-ShelfVisibility -ProcessId $process.Id -Visible $true `
        -Deadline ([DateTime]::UtcNow.AddSeconds($TimeoutSeconds))
    Assert-Style $remappedShelf $WsExTopmost "WS_EX_TOPMOST after hide/show"
    Assert-Style $remappedShelf $WsExToolWindow "WS_EX_TOOLWINDOW after hide/show"

    $shelfStyle = "0x$('{0:X8}' -f $shelf.ExStyle)"
    Write-Host "Verified shelf HWND: $($shelf.Width)x$($shelf.Height), style $shelfStyle."
    Write-Host "Verified $($edges.Count) topmost, no-activate edge HWND(s) for $monitorCount monitor(s)."
    Write-Host "Verified forwarded hide/show preserves the shelf's native topmost styles."
}
finally {
    if ($null -ne $process) {
        $process.Refresh()
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id -Force
            $process.WaitForExit()
        }
    }
    Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
}
