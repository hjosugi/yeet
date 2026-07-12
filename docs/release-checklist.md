# Release metadata checklist

The source tree targets Yeet 0.4.0. Metadata that does not depend on the final
tag is updated before tagging and is checked by `cargo metadata` and AppStream
validation.

## Complete before tagging v0.4.0

- Cargo package and lockfile package version
- Linux man-page header and AppStream release history
- Nix package version
- Inno Setup fallback version
- English and Japanese install examples

## Must remain pending until the tag exists

Do not copy hashes or commit IDs from v0.3.0 into a nominal v0.4.0 package.
After the final commit is tagged, update and verify these together:

- `packaging/arch/PKGBUILD`: set `pkgver=0.4.0`, calculate the SHA-256 of the
  actual GitHub v0.4.0 source archive, then regenerate `packaging/arch/.SRCINFO`.
- `packaging/arch/PKGBUILD-git`: regenerate its `pkgver` and `.SRCINFO-git`
  from the tagged repository rather than predicting the tag commit.
- `packaging/flatpak/io.github.hjosugi.Yeet.yml`: change the Yeet source tag
  and commit to the immutable v0.4.0 commit. Regenerate cargo sources if the
  final lockfile differs, then run a network-disabled Flatpak build.
- Any future fixed-output Nix source hash must be calculated from the final
  source. The current Nix expression consumes the repository `Cargo.lock` and
  has no release-source hash to guess; `flake.lock` pins nixpkgs and is not a
  Yeet release-version field.

Only mark these complete after building the tag artifacts and comparing their
checksums with the published release. Until then, the existing Arch and Flatpak
v0.3.0 pins intentionally remain unchanged.
