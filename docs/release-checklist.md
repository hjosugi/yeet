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

## Completed after tagging v0.4.0

The annotated v0.4.0 tag resolves to
`e63168656f6e8bb7da774495c2152d71c3bf4236`. The tag-dependent metadata was
updated and verified together:

- `packaging/arch/PKGBUILD` uses the actual GitHub v0.4.0 source archive SHA-256
  `055efb7eeb03bbf6459ef1a60f5f8f0843011c74f027275b2feb8f27b356a609`;
  `.SRCINFO` was regenerated from that PKGBUILD.
- `packaging/arch/PKGBUILD-git` and `.SRCINFO-git` use the generated version
  `0.4.0.r0.ge631686` at the tag commit.
- `packaging/flatpak/io.github.hjosugi.Yeet.yml` pins tag v0.4.0 and its full,
  immutable commit. The tagged `Cargo.lock` matches the release worktree, so
  the matching generated cargo sources remain unchanged.
- Any future fixed-output Nix source hash must be calculated from the final
  source. The current Nix expression consumes the repository `Cargo.lock` and
  has no release-source hash to guess; `flake.lock` pins nixpkgs and is not a
  Yeet release-version field.

Do not reuse these hashes or commit IDs for a later release; calculate them from
that release's tag artifacts.
