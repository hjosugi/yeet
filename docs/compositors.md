# Wayland compositor compatibility

This document separates implemented behavior from behavior that has been
confirmed on real compositor versions. Do not mark a row green from a nested
Wayland session or a compile-only CI run.

## Expected modes

| Environment | Mode | Edge drag summon | Other summon path |
|---|---|---:|---|
| sway / Hyprland / niri / river | `gtk4-layer-shell` overlay | expected | `yeet --toggle` |
| KDE Plasma (Wayland) | `gtk4-layer-shell` overlay | expected | compositor binding |
| GNOME Shell | normal-window fallback | unavailable by design | `yeet --toggle` |
| X11/XWayland platform | normal GTK window | unavailable | `yeet --toggle` |

Yeet checks `gtk4-layer-shell` protocol support at runtime. It configures
separate overlay surfaces for the always-mapped strip and shelf, with
`exclusive_zone=0`. The strip uses `KeyboardMode::None`; the shelf uses
`KeyboardMode::OnDemand`.

## Release verification matrix

Use native file-manager drags. Repeat every row at 100% and a fractional scale,
then repeat with two monitors whose scale factors differ.

| Check | sway | Hyprland | KWin | GNOME | niri | Cosmic |
|---|---:|---:|---:|---:|---:|---:|
| Strip appears on each output | ⬜ | ⬜ | ⬜ | N/A | ⬜ | ⬜ |
| Entering strip reveals shelf mid-drag | ⬜ | ⬜ | ⬜ | N/A | ⬜ | ⬜ |
| Drag continues strip → shelf and drops | ⬜ | ⬜ | ⬜ | N/A | ⬜ | ⬜ |
| Shelf opens on entered output | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Drag-out accepted removes item | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Drag-out cancelled keeps item | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Ctrl/Shift target action is honored | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| `yeet --toggle` reaches first instance | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| Output hotplug recreates strips | ⬜ | ⬜ | ⬜ | N/A | ⬜ | ⬜ |
| Mixed-scale windows stay sharp | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |

## Scripted smoke

CI starts the GTK application under both Xvfb and a headless Weston Wayland
session and proves that the UI loads and remains alive. Weston does not expose
layer-shell in this configuration, so this deliberately does not claim to test
`wl_data_device` or mid-drag layer-shell behavior. Synthetic input tools vary
by compositor and should not become a flaky release gate.

## Reporting failures

Include the compositor and version, GTK version, gtk4-layer-shell version,
output layout/scales, source application and target application. Run with
`G_MESSAGES_DEBUG=all GDK_DEBUG=events` and attach the relevant log segment.
