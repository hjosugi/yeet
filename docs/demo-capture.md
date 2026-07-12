# Reproducible demo capture

This guide defines the capture contract for README media. It does not use
synthetic drag input and it does not create placeholder screenshots. A capture
is accepted only after a reviewer watches it and confirms the checklist below.

## Asset status

<!-- markdownlint-disable MD013 -->

| Asset | Expected path | Status |
| --- | --- | --- |
| Linux dark screenshot | `docs/screenshots/yeet-linux-dark.png` | Present; re-verify against the release candidate |
| Linux light screenshot | `docs/screenshots/yeet-linux-light.png` | Not captured |
| Windows dark screenshot | `docs/screenshots/yeet-windows-dark.png` | Not captured |
| Windows light screenshot | `docs/screenshots/yeet-windows-light.png` | Not captured |
| Linux full-loop WebM/GIF | `docs/screenshots/yeet-linux-demo.webm`, `.gif` | Not captured |
| Windows full-loop WebM/GIF | `docs/screenshots/yeet-windows-demo.webm`, `.gif` | Not captured |

<!-- markdownlint-enable MD013 -->

Do not change a status to complete merely because a file exists. Confirm it was
captured from the current release candidate on the named platform and that the
full interaction is visible.

## Stable scene and full-loop script

Use a fresh, non-sensitive directory containing `alpha.txt`, `photo.png` and an
empty `Destination` folder. Use a 100% scale display, hide notifications and
unrelated windows, and crop a region that includes the file manager, screen
edge, shelf and destination. Never capture a real home directory or clipboard.

For the full loop, perform these actions without cuts:

1. Start with Yeet running and the shelf hidden.
2. Drag `alpha.txt` from the file manager into the edge strip.
3. Continue the same drag onto the revealed shelf and release it.
4. Navigate to `Destination` without holding a mouse button.
5. Drag `alpha.txt` out of the shelf and accept the drop in `Destination`.
6. Leave the frame running until the empty shelf hides.

Record a second internal review take that cancels step 5 with Esc and confirms
the item remains. This cancellation take need not be published, but it is part
of release verification. Capture snippet MIME behavior separately by dropping
demo text and an image, then dragging each into an application that accepts its
native content type.

## Linux capture

Run in a real Wayland compositor session. Nested/headless sessions do not count
as compositor verification.

Dependencies:

- screenshots: `grim` and `slurp` (unless `YEET_CAPTURE_GEOMETRY` is set);
- loop: `wf-recorder`, `ffmpeg` with `libvpx-vp9`, and optionally `slurp`;
- the current release-candidate Yeet binary and a native file manager.

Select the crop interactively:

```sh
scripts/capture-demo-linux.sh light
scripts/capture-demo-linux.sh dark
scripts/capture-demo-linux.sh loop
```

For repeatable coordinates, obtain a `slurp` geometry once and reuse it:

```sh
export YEET_CAPTURE_GEOMETRY='1200,180 620x720'
scripts/capture-demo-linux.sh loop docs/screenshots
```

The recorder runs until Ctrl+C, then produces VP9 WebM and a derived GIF. It
fails before capture when a dependency is missing and refuses to overwrite an
existing asset unless `--force` is supplied after the output directory.

## Windows capture

Use a clean Windows 10 and Windows 11 user session at 100% scale. Verify that
the shelf and edge strip remain topmost without covering security prompts.
Screenshots use the built-in .NET drawing API. Loop capture requires
`ffmpeg.exe` with both `gdigrab` and `libvpx-vp9`.

Pass explicit physical-pixel coordinates for the crop:

```powershell
./scripts/capture-demo-windows.ps1 -Mode light `
  -Left 1200 -Top 180 -Width 620 -Height 720
./scripts/capture-demo-windows.ps1 -Mode dark `
  -Left 1200 -Top 180 -Width 620 -Height 720
./scripts/capture-demo-windows.ps1 -Mode loop `
  -Left 900 -Top 100 -Width 960 -Height 900 -Duration 15
```

The script records for the requested duration and refuses to overwrite files
without `-Force`. It exits with a clear error when run outside Windows or when
the required ffmpeg device/encoder is unavailable.

## Review checklist

- The media contains no personal paths, notifications, clipboard contents or
  account names.
- The platform, theme and release-candidate version match the filename and the
  pull request description.
- The pointer, drag icon, edge reveal, accepted destination and final hide are
  visible in the full-loop take.
- The video has no edit between drag-in and drag-out; the GIF is derived from
  the same WebM.
- Text/image demonstrations use real applications and preserve the expected
  MIME behavior; a materialized fallback file alone is not sufficient proof.
- Light and dark screenshots use identical dimensions and comparable shelf
  contents.
- `ffprobe docs/screenshots/yeet-PLATFORM-demo.webm` reports VP9 video and no
  unexpected audio stream.

Only after this review should README references be added for assets currently
marked “Not captured”.
