# Windows release signing

The Windows release remains buildable without signing credentials. Unsigned tag
builds produce the same portable ZIP, Inno Setup installer and checksums, but may
trigger a SmartScreen warning.

Yeet is distributed on Windows through [Scoop](../bucket/yeet.json), the setup
EXE, the portable ZIP and `yeetup`. It is not submitted to winget: that requires
a pull request to `microsoft/winget-pkgs` gated on the Microsoft Contributor
License Agreement, which is a legal acceptance the project chose not to pursue.

## Optional Authenticode signing in GitHub Actions

Add both repository Actions secrets to enable signing:

- `WINDOWS_SIGNING_CERTIFICATE_BASE64`: base64-encoded PFX containing a
  private-key certificate with the Code Signing enhanced key usage.
- `WINDOWS_SIGNING_CERTIFICATE_PASSWORD`: the PFX password.

The release workflow rejects a half-configured pair. When both secrets exist,
it signs and verifies the bundled `yeet.exe` before creating the portable ZIP,
then signs and verifies the Inno Setup installer before calculating checksums.
The PFX file and imported certificate are removed in `finally` blocks. The
signing script uses SHA-256 file digests and an RFC 3161 SHA-256 timestamp.

To create the base64 value locally in PowerShell:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("yeet-code-signing.pfx")) |
  Set-Clipboard
```

For a local signing test on Windows with the Windows SDK installed:

```powershell
$version = "0.7.0"
$password = Read-Host "PFX password" -AsSecureString
./packaging/windows/Sign-WindowsArtifacts.ps1 `
  -CertificatePath ./yeet-code-signing.pfx `
  -CertificatePassword $password `
  -Path "./yeet-$version-windows-x64-setup.exe"
```
