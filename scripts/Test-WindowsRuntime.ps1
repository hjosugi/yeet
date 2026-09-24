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

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    public delegate IntPtr WindowProcedure(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct WindowClass
    {
        public uint Size;
        public uint Style;
        public WindowProcedure Procedure;
        public int ClassExtra;
        public int WindowExtra;
        public IntPtr Instance;
        public IntPtr Icon;
        public IntPtr Cursor;
        public IntPtr Background;
        public string MenuName;
        public string ClassName;
        public IntPtr SmallIcon;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern ushort RegisterClassExW(ref WindowClass windowClass);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateWindowExW(uint exStyle, string className, string name,
        uint style, int x, int y, int width, int height, IntPtr parent, IntPtr menu,
        IntPtr instance, IntPtr parameter);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool DestroyWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern IntPtr DefWindowProcW(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr GetModuleHandleW(string name);

    // Held in a static field so the collector never frees a procedure Windows still calls.
    private static readonly WindowProcedure DefaultProcedure = DefWindowProcW;

    // A hidden, never-activated top-level window of the given class, created by this process.
    public static IntPtr CreateInertWindow(string className)
    {
        const int ClassAlreadyExists = 1410;
        const uint WsExToolWindow = 0x00000080;
        const uint WsExNoActivate = 0x08000000;
        const uint WsPopup = 0x80000000;
        var windowClass = new WindowClass
        {
            Size = (uint)Marshal.SizeOf(typeof(WindowClass)),
            Procedure = DefaultProcedure,
            Instance = GetModuleHandleW(null),
            ClassName = className,
        };
        if (RegisterClassExW(ref windowClass) == 0 && Marshal.GetLastWin32Error() != ClassAlreadyExists)
        {
            throw new System.ComponentModel.Win32Exception();
        }
        IntPtr hwnd = CreateWindowExW(WsExToolWindow | WsExNoActivate, className, "", WsPopup,
            0, 0, 1, 1, IntPtr.Zero, IntPtr.Zero, windowClass.Instance, IntPtr.Zero);
        if (hwnd == IntPtr.Zero)
        {
            throw new System.ComponentModel.Win32Exception();
        }
        return hwnd;
    }
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

function Test-ShelfVisible([uint32]$ProcessId) {
    return @(
        Get-ProcessWindows -ProcessId $ProcessId |
            Where-Object { $_.Title -eq "Yeet" -and $_.Visible }
    ).Count -gt 0
}

function Wait-EdgesReady([System.Diagnostics.Process]$Process, [int]$MonitorCount, [DateTime]$Deadline) {
    do {
        Start-Sleep -Milliseconds 250
        $Process.Refresh()
        if ($Process.HasExited) {
            throw "yeet.exe exited early with code $($Process.ExitCode)."
        }
        $edges = @(
            Get-ProcessWindows -ProcessId $Process.Id |
                Where-Object { $_.Title -eq "Yeet edge" -and $_.Visible }
        )
        if ($edges.Count -eq $MonitorCount) {
            return
        }
    } while ([DateTime]::UtcNow -lt $Deadline)
    throw "Yeet did not map one edge per monitor before the timeout."
}

# Drive the drag-start trigger (#57) through the same event it watches for: a
# top-level window of the shell drag helper's class appearing in, then leaving,
# another process. This proves the hook, the reveal and the put-back on a real
# Windows session; a real Explorer, browser or Office drag stays a manual check.
function Test-DragSummon([string]$Path, [int]$MonitorCount) {
    $env:APPDATA = Join-Path $profileRoot "DragRoaming"
    $env:LOCALAPPDATA = Join-Path $profileRoot "DragLocal"
    New-Item -ItemType Directory -Path $env:APPDATA, $env:LOCALAPPDATA -Force | Out-Null

    $summoned = Start-Process -FilePath $Path -ArgumentList "--hidden" -PassThru
    try {
        Wait-EdgesReady $summoned $MonitorCount ([DateTime]::UtcNow.AddSeconds($TimeoutSeconds))
        if (Test-ShelfVisible $summoned.Id) {
            throw "An empty shelf started with --hidden is visible."
        }

        # Any other window appearing is not a drag.
        $unrelated = [YeetNativeWindow]::CreateInertWindow("YeetRuntimeUnrelatedWindow")
        Start-Sleep -Milliseconds 1500
        [void][YeetNativeWindow]::DestroyWindow($unrelated)
        if (Test-ShelfVisible $summoned.Id) {
            throw "An unrelated top-level window revealed the shelf."
        }

        $yeetWindows = @(Get-ProcessWindows -ProcessId $summoned.Id | ForEach-Object Handle)
        $dragImage = [YeetNativeWindow]::CreateInertWindow("SysDragImage")
        try {
            $shelf = Wait-ShelfVisibility -ProcessId $summoned.Id -Visible $true `
                -Deadline ([DateTime]::UtcNow.AddSeconds($TimeoutSeconds))
            Assert-Style $shelf $WsExTopmost "WS_EX_TOPMOST when revealed by a drag"
            Assert-Style $shelf $WsExToolWindow "WS_EX_TOOLWINDOW when revealed by a drag"
            # The drag's source keeps the keyboard focus: the reveal must not activate Yeet.
            $foreground = [YeetNativeWindow]::GetForegroundWindow()
            $yeetWindows += @(Get-ProcessWindows -ProcessId $summoned.Id | ForEach-Object Handle)
            if ($yeetWindows -contains $foreground) {
                throw "Revealing the shelf for a drag made a Yeet window the foreground window."
            }
        }
        finally {
            [void][YeetNativeWindow]::DestroyWindow($dragImage)
        }

        # Nothing was dropped, so the shelf that came out for the drag goes away with it.
        [void](Wait-ShelfVisibility -ProcessId $summoned.Id -Visible $false `
            -Deadline ([DateTime]::UtcNow.AddSeconds($TimeoutSeconds)))
        Write-Host "Verified a drag-image window reveals the hidden shelf without activating it, and its end puts the unused shelf back."
    }
    finally {
        $summoned.Refresh()
        if (-not $summoned.HasExited) {
            Stop-Process -Id $summoned.Id -Force
            $summoned.WaitForExit()
        }
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

    # One Yeet per session: stop this one before starting the drag-summon instance.
    Stop-Process -Id $process.Id -Force
    $process.WaitForExit()
    Test-DragSummon $executablePath $monitorCount
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
