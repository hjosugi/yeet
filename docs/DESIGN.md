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
- Full clipboard *manager* (Yeet captures the current clipboard on demand but
  does not keep clipboard history; competing with CopyQ is not the point).
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
| Runtime | native | Electron | GTK3 | Rust + GTK 4 |

Yoink's shelf UI (reference: real-world usage screenshots) is a narrow
vertical panel at the screen edge: thumbnail + filename per item, per-item
hover controls (remove ✕, QuickLook preview 👁, pin/unlock 🔒), and a footer
with a settings gear and a clear-all button. Yeet adopts this layout.

## 3. Architecture

**Stack: Rust (≥ 1.92) / gtk4-rs / gtk4-layer-shell, Cargo.**

Why this stack:

- **Python + PySide6** — excellent cross-platform DnD, but no safe Python API
  for assigning a Qt client surface the layer-shell role. Rejected by G3.
- **Go + Fyne** — simple distribution, but external drag-source and
  layer-shell support do not cover the core workflow. Rejected by G1/G3.
- **Electron (DropPoint's stack)** — proven drag-out via
  `webContents.startDrag()`, but no layer-shell, flaky Wayland global
  shortcuts, and a heavy runtime. Rejected by G3/G6.
- **Rust + GTK 4** — GTK/GDK maps drag-and-drop to `wl_data_device` on Wayland
  and OLE on Windows. Safe Rust bindings cover both GTK and layer-shell, so
  application and platform integration code share one implementation language.
  Chosen.

### Module layout

```
src/
  main.rs      GtkApplication lifecycle and forwarded CLI arguments
  model.rs     ShelfItem, persistence, dedupe and pin/removal rules
  ui.rs        GTK shelf/strip widgets and GDK drag-and-drop controllers
  platform.rs  layer-shell, Windows native window styles and fallback
  settings.rs  persisted user preferences
```

Platform functions expose one small surface configuration boundary:

```text
configure_shelf(window)
configure_edge(window, monitor, strip_size)
set_shelf_monitor(window, monitor)
```

Selection at startup: Wayland + layer-shell protocol available → `wayland`;
Windows → `windows`; otherwise → `fallback`. GNOME is short-circuited to
fallback because it intentionally omits the protocol; other Wayland desktops
are checked against protocol availability at runtime.

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

- **Edge strip:** `zwlr_layer_shell_v1` surface via **gtk4-layer-shell**,
  layer=`overlay`, anchored to one edge,
  `exclusive_zone=0` so it overlays without reserving space. Works on
  wlroots compositors and KWin.
- **Shelf:** a *second* layer surface anchored adjacent to the strip.
  Two surfaces (rather than resizing one mid-drag) so the active drag simply
  crosses from strip → shelf; surface-resize-during-drag is compositor
  minefield territory. **[Spike S1: validate on sway/Hyprland/KWin.]**
- **Global shortcut:** `org.freedesktop.portal.GlobalShortcuts` registers the
  toggle binding on Wayland. If the portal or backend is unavailable, a
  compositor keybinding invokes `yeet --toggle` over the single-instance IPC.
- **GNOME fallback:** GNOME Shell rejects third-party layer-shell. Fallback
  mode = no strip; summon via shortcut/CLI; shelf is a normal
  always-on-top-requested window. Documented limitation.
- **Clipboard capture:** on-demand through GDK, from the footer button (and a
  double hotkey press on Windows); Yeet does not watch clipboard history.
- **HiDPI:** GTK/GDK negotiates output scaling; verify fractional scaling per
  compositor.

### 5.3 Windows

- **Edge strip:** frameless `WS_EX_TOPMOST | WS_EX_TOOLWINDOW` window,
  4–8 px at the edge, registered as an OLE drop target through GDK's normal
  `DropTarget`. Same reveal flow as Wayland.
- **Shelf:** frameless topmost tool window, edge-snapped, rounded corners +
  dark mode via DWM attributes. `WS_EX_TOPMOST` and `HWND_TOPMOST` are
  reapplied whenever the shelf is mapped.
- **Hotkey:** Ctrl+Alt+Y via `RegisterHotKey`; a quick double press captures
  the clipboard.
- **Tray:** a native notification-area menu exposes show/hide, clipboard
  capture, clear, settings and quit; Linux uses StatusNotifierItem.
- **DPI:** per-monitor v2 manifest; multi-monitor strip placement.
- **Drag out:** GDK → OLE; verify copy/move against Explorer,
  browsers, Office. **[Spike S2: drags from/to elevated apps are blocked by
  UIPI — document, don't fight.]**
- **Autostart:** `HKCU\...\Run` key (Windows) / XDG autostart `.desktop`
  (Linux), off by default.

### 5.4 Single instance & CLI

`GtkApplication`/`GApplication` owns the application id; later invocations
forward their command line to the primary instance.
`yeet FILE… | --toggle | --clear` — dragon-style terminal integration.

## 6. Data & persistence

- Model: the Rust `ShelfModel` is the single source of truth; UI mutations
  refresh the list and hide the shelf when it becomes empty.
- Persistence: atomically replaced JSON in the platform project data directory
  (`~/.local/share` semantics on Linux, `%LOCALAPPDATA%` on Windows); items
  restored on launch; snippet temp files live in an app-owned dir and are
  garbage-collected when their item is removed.
- Settings: atomically serialized Rust settings plus a GTK dialog. Keys:
  screen edge, strip size, disabled outputs, autostart, theme, language,
  reduced motion, restore-on-launch and auto-hide (default **on**).

## 7. Packaging

| Target | Artifact |
|---|---|
| Arch | AUR-ready `yeet-shelf` and `yeet-shelf-git` PKGBUILDs in-repo |
| Any Linux | Flatpak `io.github.hjosugi.Yeet` (Flathub); note: layer-shell OK in Flatpak |
| Nix | in-repo derivation |
| Windows | Inno Setup installer + portable zip; winget metadata after signing |
| CI | GitHub Actions: Linux + Windows build on PR; artifacts on tag |

Binary name `yeet`; the Arch package name is `yeet-shelf` to avoid
collisions with unrelated tools.

## 8. Testing

- **Unit:** ShelfModel, persistence and path handling (`cargo test`).
- **Wayland integration (CI):** a headless Weston session proves the native
  Wayland backend launches and remains responsive. Real DnD stays in the
  compositor matrix because synthetic input is compositor-specific.
- **Windows:** CI build + launch smoke test; manual test plan for DnD
  against Explorer/browsers.
- **Manual matrix:** sway, Hyprland, KWin (X11-free), GNOME (fallback),
  niri × single/multi-monitor × fractional scaling; Windows 10 + 11.

## 9. Risks & spikes

| ID | Risk | Mitigation |
|---|---|---|
| S1 | Mid-drag reveal: does the drag cleanly continue from strip surface onto a newly mapped shelf surface on every compositor? | Two-surface design; prototype week 1; per-compositor fallback = pre-mapped transparent shelf |
| S2 | Windows UIPI blocks DnD with elevated apps | Document; optional elevated helper is out of scope |
| S3 | GDK drag completion/cancellation differences across compositors | Check the selected action at drag end; cover with the compositor matrix |
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
