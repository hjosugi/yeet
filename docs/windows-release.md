# Windows release signing and winget submission

The Windows release remains buildable without signing credentials. Unsigned tag
builds produce the same portable ZIP, Inno Setup installer, checksums and winget
manifests, but may trigger a SmartScreen warning.

## Optional Authenticode signing in GitHub Actions

Add both repository Actions secrets to enable signing:

- `WINDOWS_SIGNING_CERTIFICATE_BASE64`: base64-encoded PFX containing a
  private-key certificate with the Code Signing enhanced key usage.
- `WINDOWS_SIGNING_CERTIFICATE_PASSWORD`: the PFX password.

The release workflow rejects a half-configured pair. When both secrets exist,
it signs and verifies the bundled `yeet.exe` before creating the portable ZIP,
then signs and verifies the Inno Setup installer before calculating checksums or
generating winget manifests. The PFX file and imported certificate are removed
in `finally` blocks. The signing script uses SHA-256 file digests and an RFC 3161
SHA-256 timestamp.

To create the base64 value locally in PowerShell:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("yeet-code-signing.pfx")) |
  Set-Clipboard
```

For a local signing test on Windows with the Windows SDK installed:

```powershell
$password = Read-Host "PFX password" -AsSecureString
./packaging/windows/Sign-WindowsArtifacts.ps1 `
  -CertificatePath ./yeet-code-signing.pfx `
  -CertificatePassword $password `
  -Path ./yeet-0.5.2-windows-x64-setup.exe
```

## Preparing a winget-pkgs submission

Generate manifests from the final, signed installer. The installer must already
be uploaded at the release URL embedded by `New-WingetManifest.ps1`.

```powershell
$version = "0.5.2"
$installer = "./yeet-$version-windows-x64-setup.exe"
./packaging/windows/New-WingetManifest.ps1 `
  -Version $version -Installer $installer -OutputDirectory ./winget
./packaging/windows/Test-WingetManifest.ps1 `
  -Version $version -ManifestDirectory ./winget -Installer $installer
./packaging/windows/New-WingetSubmission.ps1 `
  -Version $version -ManifestDirectory ./winget -OutputDirectory ./submission
winget validate --manifest "./submission/manifests/h/hjosugi/Yeet/$version" `
  --disable-interactivity
```

Tag builds also publish `yeet-VERSION-winget-pkgs.zip` containing this exact
repository layout. Extract it at the root of a fork of
`microsoft/winget-pkgs`, run its `Tools/SandboxTest.ps1` against the version
directory, then commit, push and open a pull request manually. The Yeet release
workflow deliberately does not hold credentials for, or submit changes to, the
external repository.
