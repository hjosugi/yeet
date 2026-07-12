param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Installer,
    [string]$OutputDirectory = "."
)

$ErrorActionPreference = "Stop"
$hash = (Get-FileHash -Algorithm SHA256 $Installer).Hash
$baseUrl = "https://github.com/hjosugi/yeet/releases/download/v$Version"
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null

@"
PackageIdentifier: hjosugi.Yeet
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.10.0
"@ | Set-Content -Encoding utf8 "$OutputDirectory/hjosugi.Yeet.yaml"

@"
PackageIdentifier: hjosugi.Yeet
PackageVersion: $Version
InstallerType: inno
Scope: user
InstallModes:
- interactive
- silent
- silentWithProgress
Installers:
- Architecture: x64
  InstallerUrl: $baseUrl/$(Split-Path -Leaf $Installer)
  InstallerSha256: $hash
ManifestType: installer
ManifestVersion: 1.10.0
"@ | Set-Content -Encoding utf8 "$OutputDirectory/hjosugi.Yeet.installer.yaml"

@"
PackageIdentifier: hjosugi.Yeet
PackageVersion: $Version
PackageLocale: en-US
Publisher: hjosugi
PublisherUrl: https://github.com/hjosugi
PackageName: Yeet
PackageUrl: https://github.com/hjosugi/yeet
License: MIT
LicenseUrl: https://github.com/hjosugi/yeet/blob/v$Version/LICENSE
ShortDescription: Native Yoink-style drag-and-drop shelf for Wayland and Windows
Tags:
- drag-and-drop
- files
- utility
ReleaseNotesUrl: https://github.com/hjosugi/yeet/releases/tag/v$Version
ManifestType: defaultLocale
ManifestVersion: 1.10.0
"@ | Set-Content -Encoding utf8 "$OutputDirectory/hjosugi.Yeet.locale.en-US.yaml"
