# Yeet on GNOME

Yoink's premise is that the shelf is never buried by the window you are dragging
from. On GNOME that guarantee needs help, because Mutter implements neither
`wlr_layer_shell_v1` nor any other way for a Wayland client to raise or place
its own surfaces.

Yeet therefore picks a backend at start-up, before GTK opens a display.

| Session | Backend | How the shelf stays on top |
| --- | --- | --- |
| wlroots (sway, Hyprland, river) | `LayerShell` | `wlr_layer_shell_v1` overlay surfaces |
| GNOME with the Yeet extension | `ShellExtension` | The extension calls `make_above()` and places the windows |
| GNOME without it | `X11` | Runs through XWayland: `_NET_WM_STATE_ABOVE` shelf, `_NET_WM_WINDOW_TYPE_DOCK` edge strips |
| X11 session | `X11` | The same, natively |
| Anything else | `Plain` | Ordinary window; other windows can cover it |

Mutter is a conforming EWMH window manager for X11 clients even though it
implements none of those hints for Wayland ones, which is what makes the
XWayland path work with no setup at all.

Override the choice with `YEET_BACKEND=layer-shell|x11|extension|plain`, and
trace the desktop integration with `YEET_DEBUG=1`.

## The GNOME Shell extension

The extension is optional. Its advantage over the XWayland fallback is that Yeet
stays a native Wayland client, so it renders sharply on fractionally scaled
monitors — XWayland windows are upscaled by the compositor and look soft at,
for example, 150%.

```sh
mkdir -p ~/.local/share/gnome-shell/extensions
cp -r packaging/gnome-shell-extension/yeet@hjosugi.github.io \
  ~/.local/share/gnome-shell/extensions/
gnome-extensions enable yeet@hjosugi.github.io
```

Log out and back in, then restart Yeet. It detects the extension through
GSettings (`org.gnome.shell enabled-extensions`) and stays on Wayland instead of
switching to XWayland.

## Global shortcut

GNOME has no way for an application to grab a key directly, so the shortcut goes
through the XDG GlobalShortcuts portal. Two details are specific to GNOME:

- The portal identifies a non-sandboxed application by the systemd unit its
  process lives in. A Yeet started from a terminal inherits the terminal's unit
  and the portal answers `An app id is required`. Yeet moves itself into a
  transient `app-io.github.hjosugi.Yeet-<pid>.scope` before talking to the
  portal, so a terminal launch works like a desktop-file launch.
- GNOME parses the preferred trigger with GTK's accelerator parser
  (`<Control><Alt>y`) rather than the specification's syntax (`CTRL+ALT+y`), and
  silently leaves the shortcut unbound when parsing fails. Yeet sends whichever
  syntax the running desktop understands.

The binding appears in Settings → Keyboard once it is registered. Yeet never
calls `ListShortcuts` on GNOME: it answers with an empty list and then stops
answering `BindShortcuts` on that session altogether.

## Notification area

A stock GNOME session runs no StatusNotifierWatcher, so there is no tray icon
until an AppIndicator extension is installed. Yeet reports this once and keeps
running; if a watcher appears later it registers the icon without a restart.

Summon the shelf with `Ctrl+Alt+Y`, by dragging a file against the screen edge,
or with `yeet --toggle`.
