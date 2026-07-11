# Yeet

**A Yoink-style drag-and-drop shelf for Wayland and Windows.**

Yeet gives you a temporary "shelf" for files while you drag them around.
Drag files onto the shelf, navigate freely to the destination with your
hands off the mouse button, then drag them back out. When the last item
leaves the shelf, it disappears.

> Status: **design + scaffolding phase**. See [docs/DESIGN.md](docs/DESIGN.md)
> for the full design document and the
> [issue tracker](https://github.com/hjosugi/wayland-yeet/issues) for the
> complete work breakdown.

## Why

[Yoink](https://eternalstorms.at/yoink/mac/) solves this beautifully on
macOS, but nothing does it *natively and well* on Wayland. Existing
options are either X11-era, CLI-only ([dragon](https://github.com/mwh/dragon)),
or Electron-based ([DropPoint](https://github.com/GameGodS3/DropPoint)) with
weak Wayland integration. Yeet is a native Qt 6 app designed for
Wayland first, with Windows as an equal-priority target.

## Core behavior

1. **Summon** — a few-pixel *edge strip* lives at the edge of your screen.
   Drag files against it and the shelf slides out. Also summonable via
   global shortcut, tray icon, or `yeet <files…>` from a terminal.
2. **Hold** — drop any number of files (or text/image snippets) onto the
   shelf. Your mouse is free; go find the destination window/workspace.
3. **Release** — drag items (individually, multi-selected, or as a whole
   stack) out of the shelf into any app.
4. **Vanish** — when the last item leaves the shelf, it hides itself.

## Platform integration

| | Wayland (Linux) | Windows |
|---|---|---|
| Edge trigger | `zwlr_layer_shell_v1` via LayerShellQt | topmost frameless OLE drop-target strip |
| Shelf window | layer-shell overlay surface | frameless topmost tool window |
| Global shortcut | `org.freedesktop.portal.GlobalShortcuts` | `RegisterHotKey` |
| Drag in/out | `wl_data_device` (via Qt) | OLE `CF_HDROP` (via Qt) |
| Fallback | regular window mode (GNOME) | — |

## Building

Requires Qt ≥ 6.5 (Quick), CMake ≥ 3.21, a C++20 compiler.

```sh
cmake -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --parallel
./build/yeet
```

## Prior art & credits

- [Yoink for Mac](https://eternalstorms.at/yoink/mac/) by Eternal Storms
  Software — the original UX this project chases.
- [DropPoint](https://github.com/GameGodS3/DropPoint) — cross-platform
  Electron shelf; reference for tray/shortcut UX and drag-out handling.
- [dragon](https://github.com/mwh/dragon) — drag-and-drop source/sink
  for the terminal.

## License

MIT — see [LICENSE](LICENSE).
