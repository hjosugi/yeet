# Release metadata checklist

The source tree targets Yeet 0.5.2. Metadata that does not depend on the final
tag is updated before tagging and is checked by `cargo metadata` and AppStream
validation.

## Complete before tagging v0.5.2

- [x] Cargo package and lockfile package version
- [x] Linux man-page header and AppStream release history
- [x] Nix package version
- [x] Inno Setup fallback version
- [x] English and Japanese install examples

## Complete after tagging v0.5.2

These depend on the final tag and must be calculated from that release's
artifacts, not guessed or copied from an earlier release:

- [x] `packaging/arch/PKGBUILD` source-archive SHA-256 for `v0.5.2.tar.gz`, then
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
- [x] `scripts/check-release-metadata.sh --tagged` matches the immutable tag,
  source archive, Windows checksum file and Scoop manifest.

For the historical hashes and commit IDs used by earlier releases, see the git
history of this file.
