[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet("loop", "light", "dark")][string]$Mode,
    [Parameter(Mandatory = $true)][ValidateRange(0, 65535)][int]$Left,
    [Parameter(Mandatory = $true)][ValidateRange(0, 65535)][int]$Top,
    [Parameter(Mandatory = $true)][ValidateRange(1, 65535)][int]$Width,
    [Parameter(Mandatory = $true)][ValidateRange(1, 65535)][int]$Height,
    [ValidateRange(3, 120)][int]$Duration = 15,
    [string]$OutputDirectory = "docs/screenshots",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
if ($env:OS -ne "Windows_NT") {
    throw "This capture script requires Windows."
}

function Assert-CanWrite([string[]]$Paths) {
    foreach ($path in $Paths) {
        if ((Test-Path -LiteralPath $path) -and -not $Force) {
            throw "Refusing to overwrite '$path'. Pass -Force to replace it."
        }
    }
}

function Start-Countdown([string]$Label) {
    Write-Host "$Label starts in 3 2 1"
    Start-Sleep -Seconds 3
}

function Invoke-Checked([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "'$Command' failed with exit code $LASTEXITCODE."
    }
}

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

if ($Mode -eq "light" -or $Mode -eq "dark") {
    $output = Join-Path $OutputDirectory "yeet-windows-$Mode.png"
    Assert-CanWrite @($output)
    if ($Force -and (Test-Path -LiteralPath $output)) {
        Remove-Item -LiteralPath $output -Force
    }
    Write-Host "Confirm that Yeet is using the $Mode theme and contains only demo data."
    Start-Countdown "Screenshot"
    Add-Type -AssemblyName System.Drawing
    $bitmap = [Drawing.Bitmap]::new($Width, $Height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($Left, $Top, 0, 0, $bitmap.Size)
        $bitmap.Save($output, [Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
    Write-Host "Wrote $output"
    exit 0
}

$ffmpeg = Get-Command ffmpeg.exe -ErrorAction SilentlyContinue
if (-not $ffmpeg) {
    throw "ffmpeg.exe is required for loop capture. Install ffmpeg and add it to PATH."
}
$devices = (& $ffmpeg.Source -hide_banner -devices 2>&1 | Out-String)
if ($devices -notmatch "gdigrab") {
    throw "This ffmpeg build does not provide the gdigrab capture device."
}
$encoders = (& $ffmpeg.Source -hide_banner -encoders 2>&1 | Out-String)
if ($encoders -notmatch "libvpx-vp9") {
    throw "This ffmpeg build does not provide the libvpx-vp9 encoder."
}

$webm = Join-Path $OutputDirectory "yeet-windows-demo.webm"
$gif = Join-Path $OutputDirectory "yeet-windows-demo.gif"
Assert-CanWrite @($webm, $gif)
$overwrite = if ($Force) { "-y" } else { "-n" }

Write-Host "Perform the full loop from docs/demo-capture.md during the $Duration-second recording."
Write-Host "Synthetic drag input is intentionally not generated."
Start-Countdown "Recording"
Invoke-Checked $ffmpeg.Source @(
    "-hide_banner", "-loglevel", "warning", $overwrite,
    "-f", "gdigrab", "-framerate", "30",
    "-offset_x", "$Left", "-offset_y", "$Top",
    "-video_size", "${Width}x${Height}", "-t", "$Duration",
    "-i", "desktop", "-an", "-c:v", "libvpx-vp9",
    "-crf", "32", "-b:v", "0", "-pix_fmt", "yuv420p", $webm
)
Invoke-Checked $ffmpeg.Source @(
    "-hide_banner", "-loglevel", "warning", $overwrite, "-i", $webm,
    "-filter_complex",
    "[0:v]fps=12,split[s0][s1];[s0]palettegen=max_colors=128[p];[s1][p]paletteuse=dither=bayer",
    "-loop", "0", $gif
)
Write-Host "Wrote $webm and $gif"
