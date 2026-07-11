# Windows behavior and verification

Yeet uses GTK/GDK's Windows drag implementation plus native topmost tool-window
styles from the `windows` crate. Ctrl+Alt+Y is registered as the global shelf
toggle when it is available; pressing it twice quickly captures the clipboard.
Per-user autostart uses the standard `HKCU` Run key and is controlled in
Settings. Installer validation and code signing still require a real Windows
release machine.

Both the shelf and edge strips set `WS_EX_TOPMOST` and are placed with
`SetWindowPos(HWND_TOPMOST)`. The shelf reapplies those flags whenever it is
mapped, so hiding and showing it from the CLI cannot demote it below ordinary
windows.

## Release checklist

Run on both Windows 10 and Windows 11, without administrator elevation.

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
- `yeet --toggle` forwards to the running instance.
- Portable zip and Inno Setup install/uninstall pass on a clean user account.

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
sign the executable and installer before publishing a winget manifest.
