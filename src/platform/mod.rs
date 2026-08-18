//! Per-platform window behaviour, and the choice of which one to use.
//!
//! This module owns the decision — which backend keeps the shelf above other
//! windows on this session — and dispatches to one implementation per
//! mechanism. The implementations are deliberately separate files because they
//! share nothing but the decision: [`x11`] speaks EWMH over its own X
//! connection, [`portal`] speaks D-Bus to the desktop portal, and [`windows`]
//! calls Win32 directly.

#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "windows")]
mod registry;
// Named `win32`, not `windows`: a module called `windows` here would shadow the
// `windows` crate for every path in this file, including the console attachment
// below.
#[cfg(target_os = "windows")]
mod win32;
#[cfg(target_os = "linux")]
mod x11;

use gtk::gdk;
#[cfg(not(target_os = "windows"))]
use yeet::settings::HotkeyBinding;
use yeet::settings::{HotkeyParseError, ScreenEdge, Theme};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GlobalHotkeyError {
    Invalid(HotkeyParseError),
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Conflict {
        shortcut: String,
        previous_restored: bool,
        detail: String,
    },
    Unavailable(String),
}

impl std::fmt::Display for GlobalHotkeyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(error) => write!(formatter, "invalid shortcut: {error}"),
            Self::Conflict {
                shortcut,
                previous_restored,
                detail,
            } => write!(
                formatter,
                "{shortcut} is already in use or reserved ({detail}); previous shortcut {}",
                if *previous_restored {
                    "restored"
                } else {
                    "could not be restored"
                }
            ),
            Self::Unavailable(detail) => write!(formatter, "global shortcut unavailable: {detail}"),
        }
    }
}

#[cfg(target_os = "windows")]
static THEME_OVERRIDE: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(0);
#[cfg(target_os = "windows")]
static SHELF_EDGE: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(1);

#[cfg(target_os = "windows")]
pub fn set_theme(theme: Theme) {
    use std::sync::atomic::Ordering;

    THEME_OVERRIDE.store(
        match theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        },
        Ordering::Relaxed,
    );
}

#[cfg(not(target_os = "windows"))]
pub fn set_theme(_theme: Theme) {}

/// Reconnect stdout to the launching terminal on Windows.
///
/// The GUI subsystem (see `main.rs`) stops a background launch from flashing a
/// console window but also detaches stdout. Attaching to the parent console
/// restores `--help`/`--version` output, but only when stdout is unset: a
/// redirected pipe (CI's `yeet --version | grep`) is left alone, and a launch
/// with no parent console stays silent — no console window appears.
#[cfg(target_os = "windows")]
pub fn attach_parent_console() {
    use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };

    let missing_stdout = match unsafe { GetStdHandle(STD_OUTPUT_HANDLE) } {
        Ok(handle) => handle.is_invalid(),
        Err(_) => true,
    };
    if missing_stdout {
        let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

#[cfg(not(target_os = "windows"))]
pub fn attach_parent_console() {}

#[cfg(not(target_os = "linux"))]
use gtk::prelude::GtkWindowExt;

/// How Yeet keeps the shelf above other windows on this session.
///
/// Yoink's contract is that the shelf is never buried by the window you are
/// dragging from, so every backend here has to provide "stays on top". No
/// single Linux mechanism covers every compositor: `wlr_layer_shell_v1` is
/// absent on Mutter, and Wayland gives clients no way to raise or place their
/// own surfaces. `select_backend` therefore picks the strongest mechanism the
/// running session actually offers.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShelfBackend {
    /// `wlr_layer_shell_v1` overlay surfaces (sway, Hyprland, river, …).
    LayerShell,
    /// X11 or XWayland: `_NET_WM_WINDOW_TYPE_DOCK` strips plus an
    /// `_NET_WM_STATE_ABOVE` shelf. Mutter honours both for X11 clients even
    /// though it implements neither for Wayland ones.
    X11,
    /// Native Wayland on Mutter with the companion GNOME Shell extension
    /// raising and placing Yeet's windows from the compositor side.
    ShellExtension,
    /// No mechanism is available; the shelf is an ordinary window that other
    /// windows can cover.
    Plain,
}

#[cfg(target_os = "linux")]
pub const SHELL_EXTENSION_UUID: &str = "yeet@hjosugi.github.io";

#[cfg(target_os = "linux")]
static SHELF_BACKEND: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(u8::MAX);

#[cfg(target_os = "linux")]
impl ShelfBackend {
    const fn as_u8(self) -> u8 {
        match self {
            Self::LayerShell => 0,
            Self::X11 => 1,
            Self::ShellExtension => 2,
            Self::Plain => 3,
        }
    }

    const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::LayerShell,
            1 => Self::X11,
            2 => Self::ShellExtension,
            _ => Self::Plain,
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "layer-shell" | "layershell" | "wlr" => Some(Self::LayerShell),
            "x11" | "xwayland" => Some(Self::X11),
            "extension" | "shell-extension" | "gnome-extension" => Some(Self::ShellExtension),
            "plain" | "none" | "wayland" => Some(Self::Plain),
            _ => None,
        }
    }
}

/// Choose the shelf backend and, for the X11 path, point GDK at XWayland.
///
/// This has to run before GTK opens a display: `GDK_BACKEND` is only read when
/// the display is created, and the choice itself is made purely from
/// environment and GSettings so that no display is needed to make it.
#[cfg(target_os = "linux")]
pub fn prepare_backend() {
    let backend = select_backend();
    if backend == ShelfBackend::X11 {
        // SAFETY: called from `main` before any thread is spawned and before
        // GTK/GDK reads the environment.
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }
    SHELF_BACKEND.store(backend.as_u8(), std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(target_os = "linux"))]
pub fn prepare_backend() {}

#[cfg(target_os = "linux")]
pub fn shelf_backend() -> ShelfBackend {
    let stored = SHELF_BACKEND.load(std::sync::atomic::Ordering::Relaxed);
    if stored == u8::MAX {
        // `prepare_backend` is always called first in `main`; tests and library
        // consumers that reach this without it get the conservative answer.
        return ShelfBackend::Plain;
    }
    ShelfBackend::from_u8(stored)
}

#[cfg(target_os = "linux")]
fn select_backend() -> ShelfBackend {
    if let Ok(forced) = std::env::var("YEET_BACKEND")
        && let Some(backend) = ShelfBackend::parse(&forced)
    {
        return backend;
    }
    let x11_available = std::env::var_os("DISPLAY").is_some();
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return if x11_available {
            ShelfBackend::X11
        } else {
            ShelfBackend::Plain
        };
    }
    if !mutter_session() {
        // `gtk4_layer_shell::is_supported` needs a display, so the final word
        // stays in `layer_shell_supported`; this only records the intent.
        return ShelfBackend::LayerShell;
    }
    if shell_extension_enabled() {
        return ShelfBackend::ShellExtension;
    }
    if x11_available {
        return ShelfBackend::X11;
    }
    ShelfBackend::Plain
}

#[cfg(target_os = "linux")]
fn mutter_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktop| desktop.to_ascii_lowercase().contains("gnome"))
}

/// Report whether the user has enabled the companion GNOME Shell extension.
///
/// Read through GSettings rather than D-Bus so the answer is available before
/// the GTK main loop exists. A missing schema means the session is not GNOME
/// after all, which is a "no" rather than an error.
#[cfg(target_os = "linux")]
fn shell_extension_enabled() -> bool {
    use gio::prelude::SettingsExt;

    let Some(source) = gio::SettingsSchemaSource::default() else {
        return false;
    };
    if source.lookup("org.gnome.shell", true).is_none() {
        return false;
    }
    gio::Settings::new("org.gnome.shell")
        .value("enabled-extensions")
        .try_get::<Vec<String>>()
        .is_ok_and(|uuids| uuids.iter().any(|uuid| uuid == SHELL_EXTENSION_UUID))
}

#[cfg(target_os = "linux")]
pub fn layer_shell_supported() -> bool {
    shelf_backend() == ShelfBackend::LayerShell
        && wayland_display_available()
        && gtk4_layer_shell::is_supported()
}

/// Report whether an always-mapped edge strip can be created on this session.
///
/// The strip is what makes "drag a file at the screen edge to summon the
/// shelf" work, and it only exists where Yeet can both place a window and keep
/// it above everything else.
#[cfg(target_os = "linux")]
pub fn uses_edge_strips() -> bool {
    match shelf_backend() {
        ShelfBackend::LayerShell => layer_shell_supported(),
        ShelfBackend::X11 => x11_display_available(),
        // The strip is a plain Wayland toplevel that the companion extension
        // raises and moves to the edge from the compositor side.
        ShelfBackend::ShellExtension => true,
        ShelfBackend::Plain => false,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn uses_edge_strips() -> bool {
    true
}

/// A position the user dragged the shelf to, in device pixels.
///
/// Kept here rather than read from settings on every placement: the backends
/// place the shelf on every map and on a settle timer, and reloading the
/// settings file that often would be wasteful. Three atomics rather than a
/// lock because placement runs on the GTK main thread and must not block.
static MANUAL_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static MANUAL_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static MANUAL_SET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Record where the shelf should appear, or `None` to anchor it to the edge.
pub fn set_manual_shelf_position(position: Option<(i32, i32)>) {
    use std::sync::atomic::Ordering;

    match position {
        Some((x, y)) => {
            MANUAL_X.store(x, Ordering::Relaxed);
            MANUAL_Y.store(y, Ordering::Relaxed);
            MANUAL_SET.store(true, Ordering::Relaxed);
        }
        None => MANUAL_SET.store(false, Ordering::Relaxed),
    }
}

pub fn manual_shelf_position() -> Option<(i32, i32)> {
    use std::sync::atomic::Ordering;

    MANUAL_SET.load(Ordering::Relaxed).then(|| {
        (
            MANUAL_X.load(Ordering::Relaxed),
            MANUAL_Y.load(Ordering::Relaxed),
        )
    })
}

/// Whether the user can drag the shelf somewhere and have it reappear there.
///
/// True only where Yeet places its own windows. Under `wlr_layer_shell_v1` the
/// compositor owns placement from an anchor, and a plain Wayland client cannot
/// position itself at all, so on those backends the move handle is hidden
/// rather than offered and then ignored.
#[cfg(target_os = "linux")]
pub fn supports_manual_placement() -> bool {
    shelf_backend() == ShelfBackend::X11 && x11_display_available()
}

#[cfg(target_os = "windows")]
pub fn supports_manual_placement() -> bool {
    true
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn supports_manual_placement() -> bool {
    false
}

/// The shelf's current on-screen position in device pixels.
#[cfg(target_os = "linux")]
pub fn current_shelf_position(window: &gtk::ApplicationWindow) -> Option<(i32, i32)> {
    use glib::object::Cast;

    (shelf_backend() == ShelfBackend::X11)
        .then(|| x11::current_position(window.upcast_ref()))
        .flatten()
}

#[cfg(target_os = "windows")]
pub fn current_shelf_position(window: &gtk::ApplicationWindow) -> Option<(i32, i32)> {
    use glib::object::Cast;

    win32::current_position(window.upcast_ref())
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn current_shelf_position(_window: &gtk::ApplicationWindow) -> Option<(i32, i32)> {
    None
}

#[cfg(target_os = "linux")]
fn x11_display_available() -> bool {
    use glib::prelude::ObjectExt;

    gdk::Display::default().is_some_and(|display| display.type_().name() == "GdkX11Display")
}

#[cfg(target_os = "linux")]
pub struct GlobalHotkey {
    last_error: std::rc::Rc<std::cell::RefCell<Option<GlobalHotkeyError>>>,
}

#[cfg(target_os = "linux")]
impl GlobalHotkey {
    /// The current portal registration failure, if any.
    ///
    /// The portal binds asynchronously, so unlike the Windows backend this
    /// answer can change after `install_global_hotkey` has already returned.
    pub fn registration_error(&self) -> Option<GlobalHotkeyError> {
        self.last_error.borrow().clone()
    }

    pub fn rebind(&mut self, shortcut: &str) -> Result<String, GlobalHotkeyError> {
        HotkeyBinding::parse(shortcut)
            .map(|binding| binding.normalized().to_owned())
            .map_err(GlobalHotkeyError::Invalid)
    }
}

#[cfg(target_os = "linux")]
pub fn install_global_hotkey(shortcut: &str, callback: impl Fn() + 'static) -> GlobalHotkey {
    GlobalHotkey {
        last_error: portal::install_global_hotkey(shortcut, callback),
    }
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub struct GlobalHotkey;

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
impl GlobalHotkey {
    pub fn registration_error(&self) -> Option<GlobalHotkeyError> {
        None
    }

    pub fn rebind(&mut self, shortcut: &str) -> Result<String, GlobalHotkeyError> {
        HotkeyBinding::parse(shortcut)
            .map(|binding| binding.normalized().to_owned())
            .map_err(GlobalHotkeyError::Invalid)
    }
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn install_global_hotkey(_shortcut: &str, _callback: impl Fn() + 'static) -> GlobalHotkey {
    GlobalHotkey
}

#[cfg(target_os = "windows")]
pub use win32::GlobalHotkey;

#[cfg(target_os = "windows")]
pub fn install_global_hotkey(shortcut: &str, callback: impl Fn() + 'static) -> GlobalHotkey {
    win32::GlobalHotkey::install(shortcut, callback)
}

#[cfg(not(target_os = "linux"))]
pub fn layer_shell_supported() -> bool {
    false
}

pub fn uses_premapped_shelf() -> bool {
    layer_shell_supported()
}

#[cfg(target_os = "linux")]
pub fn set_shelf_input_enabled(window: &gtk::ApplicationWindow, enabled: bool) {
    use gtk::prelude::{NativeExt, SurfaceExt};

    if !layer_shell_supported() {
        return;
    }
    let Some(surface) = window.surface() else {
        return;
    };
    if enabled {
        surface.set_input_region(None);
    } else {
        let empty = gtk::cairo::Region::create();
        surface.set_input_region(Some(&empty));
    }
}

#[cfg(not(target_os = "linux"))]
pub fn set_shelf_input_enabled(_window: &gtk::ApplicationWindow, _enabled: bool) {}

#[cfg(target_os = "linux")]
pub fn configure_shelf(window: &gtk::ApplicationWindow, edge: ScreenEdge) {
    use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    if shelf_backend() == ShelfBackend::X11 {
        x11::configure_shelf(window, edge);
        return;
    }
    if shelf_backend() == ShelfBackend::ShellExtension {
        use gtk::prelude::GtkWindowExt;

        // The companion GNOME Shell extension raises and places this window;
        // a Wayland client cannot do either for itself.
        window.set_decorated(false);
        return;
    }
    if !layer_shell_supported() {
        return;
    }
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("yeet-shelf"));
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_margin(Edge::Top, 48);
    window.set_margin(Edge::Bottom, 48);
    window.set_exclusive_zone(0);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    update_shelf_placement(window, edge);
}

#[cfg(target_os = "windows")]
pub fn configure_shelf(window: &gtk::ApplicationWindow, edge: ScreenEdge) {
    window.set_decorated(false);
    win32::configure_shelf(window, edge);
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn configure_shelf(window: &gtk::ApplicationWindow, _edge: ScreenEdge) {
    window.set_decorated(false);
}

#[cfg(target_os = "linux")]
pub fn update_shelf_placement(window: &gtk::ApplicationWindow, edge: ScreenEdge) {
    use gtk4_layer_shell::{Edge, LayerShell};

    if shelf_backend() == ShelfBackend::X11 {
        use glib::object::Cast;

        x11::place_shelf_on_current_monitor(window.upcast_ref(), edge);
        return;
    }
    if !layer_shell_supported() {
        return;
    }
    window.set_anchor(Edge::Right, edge == ScreenEdge::Right);
    window.set_anchor(Edge::Left, edge == ScreenEdge::Left);
    window.set_margin(Edge::Right, if edge == ScreenEdge::Right { 8 } else { 0 });
    window.set_margin(Edge::Left, if edge == ScreenEdge::Left { 8 } else { 0 });
}

#[cfg(target_os = "windows")]
pub fn update_shelf_placement(window: &gtk::ApplicationWindow, edge: ScreenEdge) {
    win32::update_shelf_placement(window, edge);
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn update_shelf_placement(_window: &gtk::ApplicationWindow, _edge: ScreenEdge) {}

#[cfg(target_os = "linux")]
pub fn configure_edge(
    window: &gtk::Window,
    monitor: &gdk::Monitor,
    strip_size: i32,
    edge: ScreenEdge,
) {
    use gtk::prelude::{GtkWindowExt, MonitorExt};
    use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    if shelf_backend() == ShelfBackend::X11 {
        x11::configure_edge(window, monitor, strip_size, edge);
        return;
    }
    if shelf_backend() == ShelfBackend::ShellExtension {
        // Requested size only; the extension anchors it to the monitor edge and
        // stretches it to the work area height.
        window.set_decorated(false);
        window.set_default_size(logical_strip_size(strip_size, monitor.scale_factor()), 1);
        return;
    }
    if !layer_shell_supported() {
        return;
    }
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("yeet-edge-strip"));
    window.set_default_size(logical_strip_size(strip_size, monitor.scale_factor()), 1);
    window.set_monitor(Some(monitor));
    window.set_anchor(Edge::Right, edge == ScreenEdge::Right);
    window.set_anchor(Edge::Left, edge == ScreenEdge::Left);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_exclusive_zone(0);
    window.set_keyboard_mode(KeyboardMode::None);
}

#[cfg(target_os = "linux")]
fn logical_strip_size(physical_size: i32, scale_factor: i32) -> i32 {
    let physical_size = physical_size.clamp(3, 16);
    let scale_factor = scale_factor.max(1);
    ((physical_size + scale_factor - 1) / scale_factor).max(2)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::logical_strip_size;

    #[test]
    fn default_strip_stays_near_six_physical_pixels_at_integer_scales() {
        for scale in 1..=4 {
            let logical = logical_strip_size(6, scale);
            let physical = logical * scale;
            assert!((4..=8).contains(&physical), "scale {scale}: {physical}px");
        }
    }

    #[test]
    fn strip_size_is_clamped_before_scale_conversion() {
        assert_eq!(logical_strip_size(-10, 1), 3);
        assert_eq!(logical_strip_size(100, 1), 16);
        assert_eq!(logical_strip_size(3, 2), 2);
        assert_eq!(logical_strip_size(16, 2), 8);
        assert_eq!(logical_strip_size(6, 0), 6);
    }
}

#[cfg(target_os = "linux")]
pub fn set_shelf_monitor(
    window: &gtk::ApplicationWindow,
    monitor: &gdk::Monitor,
    edge: ScreenEdge,
) {
    use gtk4_layer_shell::LayerShell;

    if shelf_backend() == ShelfBackend::X11 {
        use glib::object::Cast;

        x11::place_shelf(window.upcast_ref(), monitor, edge);
        return;
    }
    if layer_shell_supported() {
        window.set_monitor(Some(monitor));
    }
}

#[cfg(target_os = "windows")]
pub fn refresh_window_theme(window: &gtk::Window) {
    win32::refresh_window_theme(window);
}

#[cfg(not(target_os = "windows"))]
pub fn refresh_window_theme(_window: &gtk::Window) {}

#[cfg(target_os = "windows")]
pub fn configure_edge(
    window: &gtk::Window,
    monitor: &gdk::Monitor,
    strip_size: i32,
    edge: ScreenEdge,
) {
    win32::configure_window(window, monitor, true, strip_size, edge);
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn configure_edge(
    window: &gtk::Window,
    _monitor: &gdk::Monitor,
    _strip_size: i32,
    _edge: ScreenEdge,
) {
    window.set_decorated(false);
}

#[cfg(target_os = "linux")]
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    use std::fs;

    let Some(config) = directories::BaseDirs::new() else {
        return Err(std::io::Error::other("configuration directory unavailable"));
    };
    let path = config
        .config_dir()
        .join("autostart/io.github.hjosugi.Yeet.desktop");
    if !enabled {
        return match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let executable = std::env::current_exe()?;
    let executable = executable
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    fs::write(
        path,
        format!(
            "[Desktop Entry]\nType=Application\nName=Yeet\nExec=\"{executable}\" --hidden\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        ),
    )
}

/// Add or remove Yeet's per-user `Run` entry.
///
/// Written through the registry API rather than `reg.exe`, which would flash a
/// console window over the settings dialog that triggered it.
#[cfg(target_os = "windows")]
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "Yeet";

    if !enabled {
        return registry::delete_current_user_value(RUN_KEY, VALUE);
    }
    let command = format!("\"{}\" --hidden", std::env::current_exe()?.display());
    registry::set_current_user_string(RUN_KEY, VALUE, &command)
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn set_autostart(_enabled: bool) -> std::io::Result<()> {
    Err(std::io::Error::other("autostart is not supported"))
}

#[cfg(target_os = "windows")]
pub fn set_shelf_monitor(
    window: &gtk::ApplicationWindow,
    monitor: &gdk::Monitor,
    edge: ScreenEdge,
) {
    win32::move_shelf_to_monitor(window, monitor, edge);
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn set_shelf_monitor(
    _window: &gtk::ApplicationWindow,
    _monitor: &gdk::Monitor,
    _edge: ScreenEdge,
) {
}

#[cfg(target_os = "linux")]
fn wayland_display_available() -> bool {
    use glib::prelude::ObjectExt;

    gdk::Display::default().is_some_and(|display| display.type_().name() == "GdkWaylandDisplay")
}
