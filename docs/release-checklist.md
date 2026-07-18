# Release metadata checklist

The source tree targets Yeet 0.5.1. Metadata that does not depend on the final
tag is updated before tagging and is checked by `cargo metadata` and AppStream
validation.

## Complete before tagging v0.5.1

- [x] Cargo package and lockfile package version
- [x] Linux man-page header and AppStream release history
- [x] Nix package version
- [x] Inno Setup fallback version
- [x] English and Japanese install examples

## Complete after tagging v0.5.1

These depend on the final tag and must be calculated from that release's
artifacts, not guessed or copied from an earlier release:

- [ ] `packaging/arch/PKGBUILD` source-archive SHA-256 for `v0.5.1.tar.gz`, then
  regenerate `.SRCINFO` from it.
- [ ] `packaging/arch/PKGBUILD-git` and `.SRCINFO-git` generated version at the
  tag commit.
- [ ] `packaging/flatpak/io.github.hjosugi.Yeet.yml` tag and its full, immutable
  commit. The tagged `Cargo.lock` matches the release worktree, so the matching
  generated cargo sources remain unchanged.
- [x] The Nix expression consumes the repository `Cargo.lock` and has no
  release-source hash to recalculate; `flake.lock` pins nixpkgs and is not a
  Yeet release-version field.
- [ ] The Scoop manifest in `bucket/yeet.json` is refreshed by the Scoop
  Excavator workflow once the release publishes; trigger that workflow manually
  for an immediate update.

For the historical hashes and commit IDs used by earlier releases, see the git
history of this file.
