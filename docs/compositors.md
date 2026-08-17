# Wayland compositor compatibility

This document separates implemented behavior from behavior that has been
confirmed on real compositor versions. Do not mark a row green from a nested
Wayland session or a compile-only CI run.

## Expected modes

| Environment | Mode | Edge drag summon | Other summon path |
|---|---|---:|---|
| sway / Hyprland / niri / river | `gtk4-layer-shell` overlay | expected | `yeet --toggle` |
| KDE Plasma (Wayland) | `gtk4-layer-shell` overlay | expected | compositor binding |
| GNOME Shell | XWayland dock strips + `_NET_WM_STATE_ABOVE` shelf | expected | portal shortcut, `yeet --toggle` |
| GNOME Shell + Yeet extension | native Wayland, raised and placed by the extension | expected | portal shortcut, `yeet --toggle` |
| X11 session | dock strips + `_NET_WM_STATE_ABOVE` shelf | expected | `yeet --toggle` |

Yeet checks `gtk4-layer-shell` protocol support at runtime. It configures
separate overlay surfaces for the always-mapped strip and shelf, with
`exclusive_zone=0`. The strip uses `KeyboardMode::None`; the shelf uses
`KeyboardMode::OnDemand`.

In layer-shell mode the shelf surface is mapped at startup, before any drag
begins. While hidden it is fully transparent and has an empty GDK input region,
so pointer and drag events pass through to the application below. Entering an
edge restores the existing shelf surface's full input region and reveals its
contents; it does not create or assign a new Wayland surface role during the
active drag. Normal-window fallback mode continues to unmap the hidden shelf.

GNOME is no longer a fallback-only environment. Mutter implements no
layer-shell protocol and gives Wayland clients no way to raise or place
themselves, so Yeet either runs through XWayland — where Mutter does honour
`_NET_WM_WINDOW_TYPE_DOCK` and `_NET_WM_STATE_ABOVE` — or, when the companion
GNOME Shell extension is enabled, stays a native Wayland client that the
extension raises and positions. See [Yeet on GNOME](gnome.md). The XWayland path
needs no setup but is upscaled by the compositor on fractionally scaled
monitors; the extension path is not.

The configured strip width is treated as a physical-pixel target. Yeet converts
it to a logical width using each monitor's GDK scale factor and rebuilds the
affected strips when monitor scale or geometry changes. GTK remains responsible
for fractional buffer scaling and rendering; the real-compositor matrix below
is still required before claiming a fractional or mixed-DPI configuration.

## Release verification matrix

Use native file-manager drags. Repeat every row at 100% and a fractional scale,
then repeat with two monitors whose scale factors differ.

| Check | sway | Hyprland | KWin | GNOME | niri | Cosmic |
|---|---:|---:|---:|---:|---:|---:|
| Strip appears on each output | ⬜ | ⬜ | ⬜ | 🟨 | ⬜ | ⬜ |
| Entering strip reveals shelf mid-drag | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Drag continues strip → shelf and drops | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Shelf stays above the drag source | ⬜ | ⬜ | ⬜ | 🟨 | ⬜ | ⬜ |
| Shelf opens on entered output | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Drag-out accepted removes item | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Drag-out cancelled keeps item | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Ctrl/Shift target action is honored | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Global shortcut binds and fires | ⬜ | ⬜ | ⬜ | 🟨 | ⬜ | ⬜ |
| `yeet --toggle` reaches first instance | ⬜ | ⬜ | ⬜ | ✅ | ⬜ | ⬜ |
| Output hotplug recreates strips | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Mixed-scale windows stay sharp | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |

⬜ untested ・ 🟨 partially verified ・ ✅ verified interactively

GNOME 50.4 on Wayland (single 1920×1080 output at scale 1, Arch) was checked
with the XWayland backend on 2026-08-17. Verified by inspecting the live X
windows rather than by a screenshot: the edge strip is created per output as
`_NET_WM_WINDOW_TYPE_DOCK` with `_NET_WM_STATE_ABOVE`, spanning the full output
height at the configured width, and the shelf carries `_NET_WM_STATE_ABOVE`,
`_NET_WM_STATE_STICKY` and both skip hints. `yeet --toggle` reaches the first
instance. The global shortcut is registered by GNOME
(`<Control><Alt>y` appears under
`/org/gnome/settings-daemon/global-shortcuts/io.github.hjosugi.Yeet/`), but
delivery of the activation was not exercised. The mid-drag rows still need a
real file-manager drag, and the multi-output and fractional-scale rows need
hardware that was not attached.

## Scripted smoke

CI starts the GTK application under Xvfb and a headless Weston Wayland session
to cover the normal-window fallback. It also starts a headless Sway session,
requires Yeet to request `zwlr_layer_shell_v1` surfaces, and proves that the
pre-mapped hidden shelf plus edge surfaces remain alive. The Sway smoke uses
the Cairo renderer because a headless wlroots output has no real Vulkan
presentation surface.

These tests do not inject a synthetic drag and therefore do not claim to verify
`wl_data_device` handoff or real fractional rendering. Synthetic input tools
vary by compositor and should not become a flaky release gate.

## Reporting failures

Include the compositor and version, GTK version, gtk4-layer-shell version,
output layout/scales, source application and target application. Run with
`G_MESSAGES_DEBUG=all GDK_DEBUG=events` and attach the relevant log segment.
