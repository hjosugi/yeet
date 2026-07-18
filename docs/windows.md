# Windows behavior and verification

Yeet uses GTK/GDK's Windows drag implementation plus native topmost tool-window
styles from the `windows` crate. The global shelf shortcut uses `RegisterHotKey`;
its configurable default is Ctrl+Alt+Y, and pressing the active shortcut twice
quickly captures the clipboard. Per-user autostart uses the standard `HKCU` Run
key and is controlled in Settings. Installer validation and code signing still
require a real Windows release machine.

Both the shelf and edge strips set `WS_EX_TOPMOST` and are placed with
`SetWindowPos(HWND_TOPMOST)`. The shelf reapplies those flags whenever it is
mapped, so hiding and showing it from the CLI cannot demote it below ordinary
windows. GTK 4 rebuilds extended styles on ordinary Win32 toplevels and removes
`WS_EX_TOOLWINDOW`; Yeet subclasses each native window and preserves its
tool-window/topmost bits across `WM_STYLECHANGING`, with `WS_EX_NOACTIVATE`
preserved on edge strips.

Windows CI launches the assembled portable bundle with an isolated profile and
clean `PATH`, adds a real file so the shelf maps, and enumerates its native
windows. The test requires one visible shelf and one visible edge strip per
reported monitor; it checks their dimensions and requires `WS_EX_TOPMOST` plus
`WS_EX_TOOLWINDOW`, with `WS_EX_NOACTIVATE` on every edge. This catches missing
runtime files and regressions in the native topmost setup before release. It
does not simulate Win+D, fullscreen transitions, display hotplug or an OLE drag,
so the corresponding real-machine checks below still apply.

The notification-area icon is implemented directly with `Shell_NotifyIconW`.
Its tooltip includes the current shelf item count, left-click toggles the shelf,
and its menu exposes Show/Hide, Capture Clipboard, Clear, Settings, and Quit. It
also registers for the `TaskbarCreated` message so Explorer restarts can recreate
the icon.

Settings validates and normalizes global shortcuts before registration. A
runtime change first releases the active shortcut and tries the new one. If
Windows or another application already owns it, Yeet shows a conflict error and
tries to restore the previous shortcut; the error explicitly says when that
rollback also fails. The WM_HOTKEY callback is retained during re-registration,
so double-press clipboard capture follows the newly registered shortcut.

## Release checklist

Run on both Windows 10 and Windows 11, without administrator elevation.
Windows-target compilation and CI do not replace these runtime checks; the
following tray, hotkey, focus, and installer checks remain real-machine work.

- Explorer → edge strip → shelf works for one file, many files and a folder.
- Shelf → Explorer and Desktop offers copy/move and honors Ctrl/Shift.
- Cancel with Esc and drop on an invalid target both retain the shelf item.
- Browser upload, Office, Slack and Discord accept a dragged local file.
- UNC paths, paths longer than 260 characters and available OneDrive files
  survive the shelf round trip.
- A strip exists on every monitor and opens the shelf on that monitor.
- Moving between 100%, 125%, 150% and 200% monitors stays crisp and correctly
  positioned; display hotplug recreates the strips.
- The shelf and strip have no taskbar buttons. Entering the strip during a drag
  does not take focus away from the source.
- The shelf and every edge strip remain above ordinary windows. Hide/show,
  shortcut toggles, focus changes and display changes do not demote them.
- The notification-area tooltip tracks the shelf item count, left-click toggles
  the shelf, and every menu action performs the labeled operation.
- Restarting Explorer recreates one working notification-area icon; quitting
  Yeet removes it.
- Ctrl+Alt+Y works by default. A valid replacement works immediately, the old
  shortcut stops firing, and double-press clipboard capture follows the new
  shortcut.
- Invalid shortcut text is rejected in Settings. Registering a shortcut already
  owned by another application shows a conflict, leaves the previous shortcut
  active after rollback, and clearly reports the rare rollback-failure case.
- `yeet --toggle` forwards to the running instance.
- Portable zip and Inno Setup install/uninstall pass on a clean user account.

## GTK runtime helpers in the portable bundle

`GtkApplication` registers on a D-Bus session bus for single-instance argument
forwarding (`yeet --toggle`, `yeet FILE...`). On Windows GLib autolaunches that
bus by spawning `gdbus.exe`; when the helper is missing GIO logs
`win32 session dbus binary not found` and Yeet fails to start. `ldd` only reports
the DLLs `yeet.exe` links against, so the release bundling step copies the
standalone helper executables explicitly. The portable ZIP and installer ship
`gdbus.exe` plus the GLib `gspawn` helpers (`gspawn-win64-helper.exe`,
`gspawn-win64-helper-console.exe`) next to `yeet.exe`. When assembling a bundle
by hand, copy those three files from the GTK runtime's `bin` directory alongside
`yeet.exe`, otherwise the application will not launch.

## Integrity-level limitation

Windows User Interface Privilege Isolation blocks OLE drag-and-drop across
integrity levels. A normal Yeet process cannot receive a drag from an elevated
Explorer/application, and an elevated Yeet process cannot safely solve the
opposite direction. Yeet intentionally runs `asInvoker`; no elevated helper or
`ChangeWindowMessageFilterEx` workaround is used. Match the privilege level of
the source and destination instead.

## SmartScreen and signing

Development artifacts are unsigned and can trigger SmartScreen reputation
warnings. A stable public release should use an Authenticode certificate and
sign the executable and installer before publishing a winget manifest. See the
[Windows release guide](windows-release.md) for the optional CI signing flow and
the reproducible `winget-pkgs` submission bundle.
