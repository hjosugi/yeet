[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidatePattern('^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$')][string]$Version,
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$ManifestDirectory,
    [string]$Installer,
    [switch]$RequireWinget
)

$ErrorActionPreference = "Stop"
$manifestRoot = (Resolve-Path -LiteralPath $ManifestDirectory).Path
$expectedFiles = @(
    "hjosugi.Yeet.yaml",
    "hjosugi.Yeet.installer.yaml",
    "hjosugi.Yeet.locale.en-US.yaml"
)

foreach ($file in $expectedFiles) {
    if (-not (Test-Path -LiteralPath (Join-Path $manifestRoot $file) -PathType Leaf)) {
        throw "Missing winget manifest: $file"
    }
}

$versionManifest = Get-Content -LiteralPath (Join-Path $manifestRoot $expectedFiles[0]) -Raw
$installerManifest = Get-Content -LiteralPath (Join-Path $manifestRoot $expectedFiles[1]) -Raw
$localeManifest = Get-Content -LiteralPath (Join-Path $manifestRoot $expectedFiles[2]) -Raw
$allManifests = @($versionManifest, $installerManifest, $localeManifest)

foreach ($manifest in $allManifests) {
    if ($manifest -notmatch '(?m)^PackageIdentifier: hjosugi\.Yeet\s*$') {
        throw "A manifest has an unexpected PackageIdentifier."
    }
    if ($manifest -notmatch "(?m)^PackageVersion: $([regex]::Escape($Version))\s*$") {
        throw "A manifest has an unexpected PackageVersion."
    }
}

if ($versionManifest -notmatch '(?m)^ManifestType: version\s*$' -or
    $installerManifest -notmatch '(?m)^ManifestType: installer\s*$' -or
    $localeManifest -notmatch '(?m)^ManifestType: defaultLocale\s*$') {
    throw "The winget manifests have unexpected ManifestType values."
}
if ($installerManifest -notmatch '(?m)^InstallerType: inno\s*$' -or
    $installerManifest -notmatch '(?m)^\s*-\s*Architecture: x64\s*$') {
    throw "The installer manifest must describe the x64 Inno Setup installer."
}

$urlMatch = [regex]::Match($installerManifest, '(?m)^\s*InstallerUrl:\s*(https://\S+)\s*$')
$hashMatch = [regex]::Match($installerManifest, '(?m)^\s*InstallerSha256:\s*([A-Fa-f0-9]{64})\s*$')
if (-not $urlMatch.Success -or -not $hashMatch.Success) {
    throw "InstallerUrl or InstallerSha256 is missing or invalid."
}
$expectedUrlPrefix = "https://github.com/hjosugi/yeet/releases/download/v$Version/"
if (-not $urlMatch.Groups[1].Value.StartsWith($expectedUrlPrefix, [StringComparison]::Ordinal)) {
    throw "InstallerUrl must start with '$expectedUrlPrefix'."
}

if ($Installer) {
    $installerFile = (Resolve-Path -LiteralPath $Installer).Path
    $actualHash = (Get-FileHash -LiteralPath $installerFile -Algorithm SHA256).Hash
    if ($actualHash -ne $hashMatch.Groups[1].Value) {
        throw "InstallerSha256 does not match '$installerFile'."
    }
    $installerUrlFilename = [Uri]::UnescapeDataString(([Uri]$urlMatch.Groups[1].Value).Segments[-1])
    if ((Split-Path -Leaf $installerFile) -ne $installerUrlFilename) {
        throw "InstallerUrl filename does not match the local installer."
    }
}

if ($RequireWinget) {
    $winget = Get-Command winget.exe -ErrorAction SilentlyContinue
    if (-not $winget) {
        throw "winget.exe is required but was not found."
    }
    & $winget.Source validate --manifest $manifestRoot --disable-interactivity
    if ($LASTEXITCODE -ne 0) {
        throw "winget validate failed (exit $LASTEXITCODE)."
    }
}

Write-Host "Validated winget manifests for hjosugi.Yeet $Version."
