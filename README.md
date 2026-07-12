# Yeet

[![CI](https://github.com/hjosugi/yeet/actions/workflows/ci.yml/badge.svg)](https://github.com/hjosugi/yeet/actions/workflows/ci.yml)

**A Yoink-style drag-and-drop shelf for Wayland and Windows.**

Yeet gives you a temporary "shelf" for files while you drag them around.
Drag files onto the shelf, navigate freely to the destination with your
hands off the mouse button, then drag them back out. When the last item
leaves the shelf, it disappears.

![Yeet shelf holding two files](docs/screenshots/yeet-linux-dark.png)

> Status: **0.3 native release**. The current build has the core shelf,
> file/text drop-in, multi-item drag-out, pinning, persistence, single-instance
> CLI forwarding, multi-monitor edge strips and Wayland layer-shell/fallback
> paths. See [the test matrix](docs/compositors.md) for verification status.

## Why

[Yoink](https://eternalstorms.at/yoink/mac/) solves this beautifully on
macOS, but nothing does it *natively and well* on Wayland. Existing
options are either X11-era, CLI-only ([dragon](https://github.com/mwh/dragon)),
or Electron-based ([DropPoint](https://github.com/GameGodS3/DropPoint)) with
weak Wayland integration. Yeet is a native Rust/GTK 4 app designed for
Wayland first, with Windows kept in the same codebase.

## Core behavior

1. **Summon** — a few-pixel *edge strip* lives at the edge of your screen.
   Drag files against it and the shelf slides out. Also summonable via
   global shortcut or `yeet <files…>` from a terminal.
2. **Hold** — drop any number of files (or text/image snippets) onto the
   shelf. Your mouse is free; go find the destination window/workspace.
3. **Release** — drag items (individually, multi-selected, or as a whole
   stack) out of the shelf into any app.
4. **Vanish** — when the last item leaves the shelf, it hides itself.

## Platform integration

| | Wayland (Linux) | Windows |
|---|---|---|
| Edge trigger | `zwlr_layer_shell_v1` via `gtk4-layer-shell` | topmost frameless OLE drop-target strip |
| Shelf window | layer-shell overlay surface | frameless topmost tool window |
| Global shortcut | XDG GlobalShortcuts portal, with `yeet --toggle` fallback | Ctrl+Alt+Y via `RegisterHotKey` |
| Drag in/out | `wl_data_device` (via GTK/GDK) | OLE (via GTK/GDK) |
| Fallback | regular window mode (GNOME) | — |

## Current features

- Drop files, folders and text; text becomes a managed snippet.
- Drag one item or a Ctrl-selected group back out. Cancelled drags stay on the
  shelf; accepted drops remove only unpinned items.
- Atomic shelf persistence and single-instance argument forwarding.
- `yeet FILE...`, `--toggle`, `--clear`, `--hidden` and `--help`.
- A strip on every monitor; the shelf opens on the monitor the drag entered.
- `gtk4-layer-shell` overlay surfaces where available and a documented GNOME
  shortcut/CLI fallback.
- GTK theme following, a Windows Ctrl+Alt+Y hotkey, and a Windows backend that
  reapplies `HWND_TOPMOST` whenever the shelf is shown.
- Clipboard capture, image/text preview, context actions, persistent settings,
  configurable edge width and per-user autostart.
- Full keyboard navigation and GTK accessibility metadata, English/Japanese UI,
  reduced-motion support, and a Linux StatusNotifier tray menu.

## Install on Linux

Download the current release archive and install it under `/usr/local`:

```sh
version=0.3.0
base="https://github.com/hjosugi/yeet/releases/download/v${version}"
curl -fLO "$base/yeet-${version}-linux-x86_64.tar.gz"
curl -fLO "$base/SHA256SUMS-linux.txt"
grep "yeet-${version}-linux-x86_64.tar.gz" SHA256SUMS-linux.txt | sha256sum -c -
tar -xzf "yeet-${version}-linux-x86_64.tar.gz"
root="yeet-${version}-linux-x86_64"
sudo cp -a "$root/bin/." /usr/local/bin/
sudo cp -a "$root/share/." /usr/local/share/
yeet --hidden
```

Install the GTK runtime first:

```sh
# Arch Linux
sudo pacman -S gtk4 gtk4-layer-shell

# Fedora
sudo dnf install gtk4 gtk4-layer-shell

# Ubuntu 25.10 or newer
sudo apt install libgtk-4-1 libgtk4-layer-shell0
```

Ubuntu 24.04 has GTK 4 but no `gtk4-layer-shell` package. Install the pinned
upstream library used by CI before running Yeet:

```sh
sudo apt update
sudo apt install libgtk-4-dev libwayland-dev wayland-protocols meson ninja-build
git clone --depth 1 --branch v1.3.0 https://github.com/wmww/gtk4-layer-shell.git /tmp/gtk4-layer-shell
meson setup /tmp/gtk4-layer-shell/build /tmp/gtk4-layer-shell \
  --prefix=/usr/local -Dexamples=false -Ddocs=false -Dtests=false \
  -Dintrospection=false -Dvapi=false
ninja -C /tmp/gtk4-layer-shell/build
sudo ninja -C /tmp/gtk4-layer-shell/build install
sudo ldconfig
```

The release archive currently targets x86-64. Arch users can alternatively
build `packaging/arch/PKGBUILD`; Nix users can run
`nix run github:hjosugi/yeet`.

## Build from source

Requires Rust ≥ 1.92, GTK ≥ 4.10 and, on Wayland,
`gtk4-layer-shell`. Install the development packages provided by your
distribution. Ubuntu 24.04 does not package the GTK4 version of layer-shell;
the CI workflow shows the pinned upstream source-build commands used there.

```sh
cargo build --release
cargo test
./target/release/yeet --hidden
```

At runtime Yeet checks whether layer-shell is supported. If it is unavailable,
the shelf uses a regular window and no edge strip is created. Bind
`yeet --toggle` in the compositor configuration for that fallback. Windows
builds use the UCRT64 GTK package in MSYS2; CI contains the exact setup.

PDF previews use `pdftoppm` when Poppler is installed and otherwise open in
the system's default PDF application.

See [Wayland compositor verification](docs/compositors.md) and
[Windows behavior and limitations](docs/windows.md) before filing a
platform-specific bug.

## Prior art & credits

- [Yoink for Mac](https://eternalstorms.at/yoink/mac/) by Eternal Storms
  Software — the original UX this project chases.
- [DropPoint](https://github.com/GameGodS3/DropPoint) — cross-platform
  Electron shelf; reference for tray/shortcut UX and drag-out handling.
- [dragon](https://github.com/mwh/dragon) — drag-and-drop source/sink
  for the terminal.

## License

MIT — see [LICENSE](LICENSE).
