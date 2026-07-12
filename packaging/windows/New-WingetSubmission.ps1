[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidatePattern('^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$')][string]$Version,
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$ManifestDirectory,
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$OutputDirectory
)

$ErrorActionPreference = "Stop"
& "$PSScriptRoot/Test-WingetManifest.ps1" -Version $Version -ManifestDirectory $ManifestDirectory

$destination = Join-Path $OutputDirectory "manifests/h/hjosugi/Yeet/$Version"
if (Test-Path -LiteralPath $destination) {
    Remove-Item -LiteralPath $destination -Recurse -Force
}
New-Item -ItemType Directory -Path $destination -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $ManifestDirectory "hjosugi.Yeet.yaml") -Destination $destination
Copy-Item -LiteralPath (Join-Path $ManifestDirectory "hjosugi.Yeet.installer.yaml") -Destination $destination
Copy-Item -LiteralPath (Join-Path $ManifestDirectory "hjosugi.Yeet.locale.en-US.yaml") -Destination $destination

Write-Host "Staged winget-pkgs submission at '$destination'."
