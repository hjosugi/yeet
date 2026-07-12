# Flathub submission checklist

The application ID is `io.github.hjosugi.Yeet`. The Flathub repository must
therefore be named `io.github.hjosugi.Yeet`, with
`io.github.hjosugi.Yeet.yml` and `cargo-sources.json` at its top level.

## Sandbox permissions

The manifest deliberately grants only the following runtime permissions:

- `--socket=wayland` lets GTK create native Wayland windows and is required
  for the layer-shell surface.
- `--socket=fallback-x11` keeps the regular-window fallback usable in an X11
  session. It does not expose X11 when Wayland is available.
- `--device=dri` enables GTK's hardware-accelerated renderer.
- `--filesystem=host:ro` lets the shelf retain, preview, and drag out files
  from arbitrary user-selected locations, including removable media. Yeet is
  a file hand-off tool rather than a document editor, so it does not need
  write access to source files. Files created from text or clipboard content,
  along with settings and shelf state, stay in the app's own XDG sandbox
  directories.

No network, audio, broad D-Bus, background, or writable host-filesystem
permission is requested. Flathub's linter requires an explanation for
read-only host access; use the `host:ro` rationale above in the submission.

## Before submitting

- [ ] Publish a signed release tag containing the final Flatpak manifest,
      desktop file, icon, and MetaInfo, then update both `tag` and `commit` in
      the manifest to that immutable release.
- [ ] Regenerate `cargo-sources.json` from the release's `Cargo.lock` with
      `flatpak-cargo-generator.py` and confirm that the build works offline.
- [ ] Add at least one current Linux screenshot to the MetaInfo. Use a direct
      HTTPS image URL pinned to a release tag or commit, with an English
      caption; screenshots are mandatory for graphical Flathub apps.
- [ ] Copy `io.github.hjosugi.Yeet.yml` and `cargo-sources.json` to the root of
      a clean `flathub/io.github.hjosugi.Yeet` submission repository.
- [ ] Build and install from the clean submission checkout:

  ```sh
  flatpak-builder --force-clean --user --install-deps-from=flathub \
    --repo=repo build-dir io.github.hjosugi.Yeet.yml
  flatpak build-bundle repo yeet.flatpak io.github.hjosugi.Yeet
  flatpak install --user --reinstall ./yeet.flatpak
  flatpak run io.github.hjosugi.Yeet --hidden
  ```

- [ ] Exercise file and text drops, previews, multi-item drag-out, persistence,
      layer-shell behavior, and the regular-window fallback inside the sandbox.
- [ ] Run the Flathub linter and resolve every error and warning:

  ```sh
  flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest \
    io.github.hjosugi.Yeet.yml
  flatpak run --command=flatpak-builder-lint org.flatpak.Builder appstream \
    io.github.hjosugi.Yeet.metainfo.xml
  ```

- [ ] Submit the manifest repository through Flathub's new-app pull-request
      process and include the sandbox-permission rationale above.
