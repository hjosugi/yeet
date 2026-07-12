[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string[]]$Path,
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$CertificatePath,
    [Parameter(Mandatory = $true)][SecureString]$CertificatePassword,
    [ValidatePattern('^https?://')][string]$TimestampUrl = "http://timestamp.digicert.com"
)

$ErrorActionPreference = "Stop"

function Find-SignTool {
    $command = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    $kitsRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits/10/bin"
    $candidate = Get-ChildItem -Path $kitsRoot -Filter signtool.exe -File -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '[\\/]x64[\\/]signtool\.exe$' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $candidate) {
        throw "signtool.exe was not found. Install the Windows 10/11 SDK."
    }
    return $candidate.FullName
}

$certificateFile = (Resolve-Path -LiteralPath $CertificatePath).Path
$artifacts = @($Path | ForEach-Object { (Resolve-Path -LiteralPath $_).Path })
$signTool = Find-SignTool
$storePath = "Cert:\CurrentUser\My"
$existingThumbprints = @(
    Get-ChildItem $storePath -ErrorAction SilentlyContinue | ForEach-Object Thumbprint
)
$importedCertificates = @()

try {
    $importedCertificates = @(
        Import-PfxCertificate -FilePath $certificateFile -CertStoreLocation $storePath `
            -Password $CertificatePassword -Exportable:$false
    )
    $signingCertificate = $importedCertificates |
        Where-Object {
            $_.HasPrivateKey -and
            ($_.EnhancedKeyUsageList.ObjectId.Value -contains "1.3.6.1.5.5.7.3.3")
        } |
        Select-Object -First 1
    if (-not $signingCertificate) {
        throw "The PFX does not contain a private-key certificate valid for code signing."
    }

    foreach ($artifact in $artifacts) {
        & $signTool sign /sha1 $signingCertificate.Thumbprint /s My /fd SHA256 `
            /tr $TimestampUrl /td SHA256 /d Yeet $artifact
        if ($LASTEXITCODE -ne 0) {
            throw "SignTool failed to sign '$artifact' (exit $LASTEXITCODE)."
        }

        & $signTool verify /pa /all /tw /v $artifact
        if ($LASTEXITCODE -ne 0) {
            throw "SignTool could not verify '$artifact' (exit $LASTEXITCODE)."
        }
    }
}
finally {
    foreach ($certificate in $importedCertificates) {
        if ($certificate.Thumbprint -notin $existingThumbprints) {
            Remove-Item -LiteralPath "$storePath/$($certificate.Thumbprint)" -Force -ErrorAction SilentlyContinue
        }
    }
}
