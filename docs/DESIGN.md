# Yeet — Design Document

A native drag-and-drop shelf ("Yoink alternative") that works *perfectly* on
Wayland and Windows.

## 1. Goals

- **G1 — Drag-summoned shelf.** While the user is dragging files, the shelf
  becomes reachable at a screen edge; dropping files parks them there.
- **G2 — Vanish when empty.** When the last item is dragged out (or removed),
  the shelf hides itself. This is the defining behavior of the app.
- **G3 — Wayland-native.** First-class behavior on wlroots compositors
  (sway, Hyprland, river, niri) and KWin, without X11/XWayland hacks.
- **G4 — Windows-native.** The same UX on Windows 10/11, not a port
  afterthought: correct DPI, dark mode, tray, hotkey, Explorer drag semantics.
- **G5 — Finder/Explorer-consistent drag semantics.** Dragging out behaves
  like the platform file manager: default action decided by the drop target,
  with modifier keys forcing copy/move.
- **G6 — Lightweight.** Native toolkit, no bundled browser runtime, near-zero
  idle cost.

### Non-goals (v1)

- macOS support (Yoink already exists there).
- Full clipboard *manager* (a lightweight clipboard-capture is planned
  post-MVP; competing with CopyQ is not the point).
- Cloud sync / Handoff-style device transfer.

## 2. Prior art

| | Yoink (macOS) | DropPoint (Electron) | dragon (GTK3 CLI) | **Yeet** |
|---|---|---|---|---|
| Summon on drag | ✅ global drag detection | ❌ manual/shortcut | ❌ CLI | ✅ edge strip (see §5) |
| Hide when empty | ✅ | ❌ | option (`--and-exit`) | ✅ core requirement |
| Wayland | — | ⚠️ weak (Chromium) | ⚠️ X11-first | ✅ layer-shell |
| Windows | — | ✅ | ❌ | ✅ |
| Stack / multi-drag | ✅ | ✅ | ✅ | ✅ |
| Non-file snippets | ✅ | ❌ | ❌ | ✅ (text/images) |
| Runtime | native | Electron | GTK3 | Qt 6 native |

Yoink's shelf UI (reference: real-world usage screenshots) is a narrow
vertical panel at the screen edge: thumbnail + filename per item, per-item
hover controls (remove ✕, QuickLook preview 👁, pin/unlock 🔒), and a footer
with a settings gear and a clear-all button. Yeet adopts this layout.

## 3. Architecture

**Stack: C++20 / Qt 6 (≥ 6.5) / QML, CMake.**

Why Qt over the alternatives considered:

- **GTK4 + gtk4-layer-shell** — excellent on Wayland, but second-class and
  painful to ship on Windows. Rejected by G4.
- **Electron (DropPoint's stack)** — proven drag-out via
  `webContents.startDrag()`, but no layer-shell, flaky Wayland global
  shortcuts, and a heavy runtime. Rejected by G3/G6.
- **Qt 6** — `QDrag`/`QMimeData` map to `wl_data_device` on Wayland and OLE
  (`CF_HDROP`) on Windows; LayerShellQt provides `zwlr_layer_shell_v1`;
  QML gives cheap polished UI on both targets. Chosen.

### Module layout

```
src/
  core/       ShelfModel, ShelfItem, persistence, single-instance, CLI
  ui/         QML engine setup, DragOutHelper, icon/thumbnail provider
  platform/
    wayland/  LayerShellQt strip + shelf surfaces, portal shortcuts   (Linux only)
    windows/  topmost strip, RegisterHotKey, DPI, dark title-bar      (Windows only)
    fallback/ plain-window mode (GNOME, unknown compositors)
qml/          Main.qml, ShelfView.qml, EdgeStrip.qml, delegates
```

Platform backends implement one interface:

```cpp
class TriggerBackend {
    virtual void installEdgeStrip(Edge edge, const OutputSet &outputs) = 0;
    virtual void showShelf(SummonReason reason) = 0;   // DragHover | Hotkey | Cli | Tray
    virtual void hideShelf() = 0;                      // called when model empties
};
```

Selection at startup: Wayland + layer-shell available → `wayland`;
Windows → `windows`; otherwise → `fallback`.

## 4. Core UX specification

### Shelf lifecycle

```
hidden ──(drag hovers edge strip / hotkey / CLI / tray)──▶ visible
visible ──(last item leaves shelf)──▶ hidden          [G2]
visible ──(Esc / focus loss with 0 items)──▶ hidden
visible + pinned items ──▶ stays visible until unpinned & empty
```

### Items

- An item is a **URL list entry**: local file/dir URL, or a *snippet*
  (text/image dropped from an app) materialized as a temp file.
- Duplicates (same URL) are not added twice.
- Item UI: thumbnail (images) or file-type icon, filename (elided middle),
  hover controls: **✕ remove · 👁 preview · 🔒 pin** (pinned items survive
  drag-out: dragging a pinned item copies it and keeps it on the shelf).
- Footer: item count, ⚙ settings, 🗑 clear-all.

### Drag in

- Accept `text/uri-list` (files from any file manager / browser downloads),
  plain text, and images. Multi-file drops create one item per file.
- Drop feedback: highlight + insertion animation.

### Drag out

- Single item, multi-selection (Ctrl/Shift-click), or **stack mode**
  (grab the stack header to drag *all* items as one `text/uri-list`).
- Offered actions: `Copy | Move`; the drop target picks. Modifiers follow
  platform conventions (Ctrl=copy on Windows/Linux file managers).
- On a completed drop (action ≠ ignore), the item is removed from the shelf
  unless pinned. When the count hits 0 → hide (G2).

## 5. Platform strategy

### 5.1 The core problem: detecting a drag globally

macOS lets Yoink observe global drags. **Wayland forbids this by design**
(clients only see drags that enter their own surfaces), and Windows has no
public global-drag hook either. The portable answer:

> Keep a **tiny always-present edge strip** (4–8 px) owned by Yeet at a
> screen edge. During any DnD, the moment the user drags onto the strip the
> compositor/OS delivers `drag-enter` to us — we then reveal the shelf right
> next to it, and the user drops onto the shelf. The strip *is* the global
> drag detector, implemented with only public APIs on both platforms.

### 5.2 Wayland (Linux)

- **Edge strip:** `zwlr_layer_shell_v1` surface via **LayerShellQt**
  (build-time optional dependency), layer=`overlay`, anchored to one edge,
  `exclusive_zone=0` so it overlays without reserving space. Works on
  wlroots compositors and KWin.
- **Shelf:** a *second* layer surface anchored adjacent to the strip.
  Two surfaces (rather than resizing one mid-drag) so the active drag simply
  crosses from strip → shelf; surface-resize-during-drag is compositor
  minefield territory. **[Spike S1: validate on sway/Hyprland/KWin.]**
- **Global shortcut:** `org.freedesktop.portal.GlobalShortcuts` (works on
  KDE, Hyprland, sway ≥ portal support); fallback: compositor keybinding
  invoking `yeet --toggle` over the single-instance IPC.
- **GNOME fallback:** GNOME Shell rejects third-party layer-shell. Fallback
  mode = no strip; summon via shortcut/CLI/tray; shelf is a normal
  always-on-top-requested window. Documented limitation.
- **Clipboard capture (post-MVP):** `ext-data-control-v1` /
  `zwlr_data_control_manager_v1` for watching the clipboard without focus.
- **HiDPI:** fractional-scale-v1 comes free with Qt ≥ 6.5; verify per
  compositor.

### 5.3 Windows

- **Edge strip:** frameless `WS_EX_TOPMOST | WS_EX_TOOLWINDOW` window,
  4–8 px at the edge, registered as an OLE drop target (Qt does this via
  the normal `DropArea`). Same reveal flow as Wayland.
- **Shelf:** frameless topmost tool window, edge-snapped, rounded corners +
  dark mode via DWM attributes.
- **Hotkey:** `RegisterHotKey`; **Tray:** `QSystemTrayIcon` (also used on
  Linux via StatusNotifier).
- **DPI:** per-monitor v2 manifest; multi-monitor strip placement.
- **Drag out:** Qt → OLE `CF_HDROP`; verify copy/move against Explorer,
  browsers, Office. **[Spike S2: drags from/to elevated apps are blocked by
  UIPI — document, don't fight.]**
- **Autostart:** `HKCU\...\Run` key (Windows) / XDG autostart `.desktop`
  (Linux), off by default.

### 5.4 Single instance & CLI

First instance wins; later invocations forward args over `QLocalSocket`
(named pipe on Windows, Unix socket on Linux).
`yeet FILE… | --toggle | --clear` — dragon-style terminal integration.

## 6. Data & persistence

- Model: `ShelfModel : QAbstractListModel` — the single source of truth;
  emits `becameEmpty()` → backend `hideShelf()`.
- Persistence: JSON at `QStandardPaths::AppDataLocation`
  (`~/.local/state` semantics on Linux, `%APPDATA%` on Windows); items
  restored on launch; snippet temp files live in an app-owned dir and are
  garbage-collected when their item is removed.
- Settings: `QSettings` + a QML settings dialog. Keys: edge, outputs,
  strip size, summon methods, autostart, theme, restore-on-launch,
  auto-hide (default **on**).

## 7. Packaging

| Target | Artifact |
|---|---|
| Arch | AUR `yeet-shelf` (PKGBUILD in-repo) |
| Any Linux | Flatpak `io.github.hjosugi.Yeet` (Flathub); note: layer-shell OK in Flatpak |
| Nix | flake.nix |
| Windows | Inno Setup installer + `winget` manifest; portable zip |
| CI | GitHub Actions: Linux + Windows build on PR; artifacts on tag |

Binary name `yeet` (note: an unrelated AUR pacman wrapper uses the name —
hence AUR package name `yeet-shelf`).

## 8. Testing

- **Unit:** ShelfModel, persistence, URL/mime handling (Qt Test, CTest).
- **Wayland integration (CI):** headless sway/cage + `wtype`/`ydotool`
  scripted DnD where feasible; else scripted smoke: launch, IPC summon,
  screenshot compare. Exploratory — tracked as a spike.
- **Windows:** CI build + launch smoke test; manual test plan for DnD
  against Explorer/browsers.
- **Manual matrix:** sway, Hyprland, KWin (X11-free), GNOME (fallback),
  niri × single/multi-monitor × fractional scaling; Windows 10 + 11.

## 9. Risks & spikes

| ID | Risk | Mitigation |
|---|---|---|
| S1 | Mid-drag reveal: does the drag cleanly continue from strip surface onto a newly mapped shelf surface on every compositor? | Two-surface design; prototype week 1; per-compositor fallback = pre-mapped transparent shelf |
| S2 | Windows UIPI blocks DnD with elevated apps | Document; optional elevated helper is out of scope |
| S3 | `QDrag::exec()` quirks on Wayland (nested loop, cancel events) across compositors | Covered by test matrix; upstream Qt bugs get minimal repros |
| S4 | GNOME has no layer-shell for third parties | Fallback mode is a first-class citizen, not an afterthought |
| S5 | GlobalShortcuts portal absent on some compositors | Always ship `yeet --toggle` IPC path for native keybinds |

## 10. Milestones

- **M0 Foundation** — scaffold, CI (Linux+Windows), single-instance, identity.
- **M1 Core shelf** — model, shelf UI, drop-in, drag-out, auto-hide-on-empty,
  multi-select, stack, pin, context menu, persistence, snippets, CLI.
- **M2 Wayland** — layer-shell strip + shelf, S1 spike, multi-monitor,
  portal shortcuts, GNOME fallback, compositor matrix.
- **M3 Windows** — strip, shelf chrome, hotkey, tray, DPI, Explorer
  semantics, autostart, S2 spike.
- **M4 Settings & polish** — settings UI, theming, i18n (en/ja), animations,
  accessibility, clipboard capture.
- **M5 Packaging & release** — Flatpak/AUR/Nix/Inno+winget, release CI,
  demo media, v1.0 checklist.

The complete work breakdown lives in the GitHub issues, one issue per task,
labeled by area and milestone.
