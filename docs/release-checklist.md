# Release metadata checklist

The source tree targets Yeet 0.6.0. Metadata that does not depend on the final
tag is updated before tagging and is checked by `cargo metadata` and AppStream
validation.

## Complete before tagging v0.6.0

- [x] Cargo package and lockfile package version, including the `xtask` and
  `yeetup` workspace members
- [x] Linux man-page header and AppStream release history
- [x] Nix package version
- [x] Inno Setup fallback version
- [x] English and Japanese install examples, including the AppImage and
  `yeetup` download names, which embed the version

## Complete after tagging v0.6.0

These depend on the final tag and must be calculated from that release's
artifacts, not guessed or copied from an earlier release:

- [x] `packaging/arch/PKGBUILD` source-archive SHA-256 for `v0.6.0.tar.gz`, then
  regenerate `.SRCINFO` from it.
- [x] `packaging/arch/PKGBUILD-git` and `.SRCINFO-git` generated version at the
  tag commit.
- [x] `packaging/flatpak/io.github.hjosugi.Yeet.yml` tag and its full, immutable
  commit. The tagged `Cargo.lock` matches the release worktree, so the matching
  generated cargo sources remain unchanged.
- [x] The Nix expression consumes the repository `Cargo.lock` and has no
  release-source hash to recalculate; `flake.lock` pins nixpkgs and is not a
  Yeet release-version field.
- [x] The Scoop manifest in `bucket/yeet.json` is refreshed from the published
  portable ZIP checksum; the Scoop Excavator workflow verifies later updates.
- [ ] `scripts/check-release-metadata.sh --tagged` matches the immutable tag,
  source archive, Windows checksum file and Scoop manifest. Every assertion it
  makes was verified by hand for v0.6.0 — the source archive was downloaded and
  hashed, and the Scoop hash was taken from the published
  `SHA256SUMS-windows.txt` — but the script itself has not completed a run,
  because GitHub kept answering the archive download with HTTP 429. Re-run it
  once the limit clears.

Yeet is no longer submitted to winget; see the closed issue #44 and
[the Windows release guide](windows-release.md).

For the historical hashes and commit IDs used by earlier releases, see the git
history of this file.
