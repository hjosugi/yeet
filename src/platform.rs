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
        last_error: linux_impl::install_global_hotkey(shortcut, callback),
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
pub use windows_impl::GlobalHotkey;

#[cfg(target_os = "windows")]
pub fn install_global_hotkey(shortcut: &str, callback: impl Fn() + 'static) -> GlobalHotkey {
    windows_impl::GlobalHotkey::install(shortcut, callback)
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
        x11_impl::configure_shelf(window, edge);
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
    windows_impl::configure_shelf(window, edge);
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

        x11_impl::place_shelf_on_current_monitor(window.upcast_ref(), edge);
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
    windows_impl::update_shelf_placement(window, edge);
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
        x11_impl::configure_edge(window, monitor, strip_size, edge);
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

        x11_impl::place_shelf(window.upcast_ref(), monitor, edge);
        return;
    }
    if layer_shell_supported() {
        window.set_monitor(Some(monitor));
    }
}

#[cfg(target_os = "windows")]
pub fn refresh_window_theme(window: &gtk::Window) {
    windows_impl::refresh_window_theme(window);
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
    windows_impl::configure_window(window, monitor, true, strip_size, edge);
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

#[cfg(target_os = "windows")]
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    use std::process::Command;

    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let status = if enabled {
        let value = format!("\"{}\" --hidden", std::env::current_exe()?.display());
        Command::new("reg")
            .args(["add", key, "/v", "Yeet", "/t", "REG_SZ", "/d", &value, "/f"])
            .status()?
    } else {
        Command::new("reg")
            .args(["delete", key, "/v", "Yeet", "/f"])
            .status()?
    };
    status
        .success()
        .then_some(())
        .ok_or_else(|| std::io::Error::other("failed to update Windows startup registration"))
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
    windows_impl::move_shelf_to_monitor(window, monitor, edge);
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

/// X11 (and XWayland) shelf placement.
///
/// Mutter refuses Wayland clients any say over stacking or position, but it is
/// a conforming EWMH window manager for X11 clients: it honours
/// `_NET_WM_WINDOW_TYPE_DOCK` for the edge strips and `_NET_WM_STATE_ABOVE`
/// for the shelf. Running Yeet's GTK windows through XWayland is therefore the
/// only way to get Yoink's "never buried" guarantee on a stock GNOME session.
///
/// GTK 4 exposes no window positioning at all, so geometry is applied straight
/// to the X server over a second connection. Owning that connection separately
/// from GDK's keeps this independent of GDK's own X11 bookkeeping; every call
/// degrades to a no-op when the connection or the surface is missing.
#[cfg(target_os = "linux")]
mod x11_impl {
    use super::ScreenEdge;
    use gtk::gdk;
    use gtk::prelude::*;
    use std::cell::OnceCell;
    use std::sync::atomic::{AtomicI8, Ordering};
    use std::time::Duration;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask, PropMode,
        Window,
    };
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;

    /// Used before GTK has allocated the window, when its real width is not
    /// known yet.
    use yeet::settings::SHELF_WIDTH;

    const SHELF_MARGIN: i32 = 8;
    const SHELF_VERTICAL_INSET: i32 = 96;
    const SHELF_MAX_HEIGHT: i32 = 560;
    /// GTK can keep adjusting X geometry after emitting `map`, so placement is
    /// applied once synchronously and once more after GDK has settled.
    const SETTLE_DELAY: Duration = Duration::from_millis(100);

    static SHELF_EDGE: AtomicI8 = AtomicI8::new(1);

    x11rb::atom_manager! {
        Atoms: AtomsCookie {
            _NET_WM_STATE,
            _NET_WM_STATE_ABOVE,
            _NET_WM_STATE_STICKY,
            _NET_WM_STATE_SKIP_TASKBAR,
            _NET_WM_STATE_SKIP_PAGER,
            _NET_WM_WINDOW_TYPE,
            _NET_WM_WINDOW_TYPE_DOCK,
            _NET_WM_WINDOW_TYPE_UTILITY,
        }
    }

    struct Session {
        connection: RustConnection,
        root: Window,
        atoms: Atoms,
    }

    thread_local! {
        static SESSION: OnceCell<Option<Session>> = const { OnceCell::new() };
    }

    fn with_session<R>(action: impl FnOnce(&Session) -> R) -> Option<R> {
        SESSION.with(|cell| cell.get_or_init(open_session).as_ref().map(action))
    }

    fn open_session() -> Option<Session> {
        let (connection, screen) = match x11rb::connect(None) {
            Ok(connected) => connected,
            Err(error) => {
                eprintln!("yeet: X11 placement unavailable: {error}");
                return None;
            }
        };
        let root = connection.setup().roots.get(screen)?.root;
        let atoms = match Atoms::new(&connection).map(|cookie| cookie.reply()) {
            Ok(Ok(atoms)) => atoms,
            Ok(Err(error)) => {
                eprintln!("yeet: X11 atoms unavailable: {error}");
                return None;
            }
            Err(error) => {
                eprintln!("yeet: X11 atoms unavailable: {error}");
                return None;
            }
        };
        Some(Session {
            connection,
            root,
            atoms,
        })
    }

    fn xid(window: &gtk::Window) -> Option<Window> {
        let surface = window.surface()?;
        let surface = surface.downcast::<gdk4_x11::X11Surface>().ok()?;
        Some(surface.xid() as Window)
    }

    /// Write an EWMH atom-list property. Effective only before the window is
    /// mapped; `send_state` covers it afterwards.
    fn set_atoms(session: &Session, window: Window, property: u32, values: &[u32]) {
        let _ = session.connection.change_property32(
            PropMode::REPLACE,
            window,
            property,
            AtomEnum::ATOM,
            values,
        );
    }

    /// Ask the window manager to add two `_NET_WM_STATE` atoms to an already
    /// mapped window. EWMH carries two per message, so pass `0` for the unused
    /// slot.
    fn send_state(session: &Session, window: Window, first: u32, second: u32) {
        const ADD: u32 = 1;
        const SOURCE_APPLICATION: u32 = 1;

        let event = ClientMessageEvent::new(
            32,
            window,
            session.atoms._NET_WM_STATE,
            [ADD, first, second, SOURCE_APPLICATION, 0],
        );
        let _ = session.connection.send_event(
            false,
            session.root,
            EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
            event,
        );
    }

    fn set_edge(edge: ScreenEdge) {
        SHELF_EDGE.store(
            if edge == ScreenEdge::Right { 1 } else { 0 },
            Ordering::Relaxed,
        );
    }

    fn current_edge() -> ScreenEdge {
        if SHELF_EDGE.load(Ordering::Relaxed) == 0 {
            ScreenEdge::Left
        } else {
            ScreenEdge::Right
        }
    }

    /// Monitor rectangle in X device pixels.
    ///
    /// GDK reports monitor geometry in application pixels; the X server works
    /// in device pixels, so everything is scaled up before it crosses over.
    fn device_area(monitor: &gdk::Monitor) -> (i32, i32, i32, i32, i32) {
        let scale = monitor.scale_factor().max(1);
        let area = monitor.geometry();
        (
            area.x() * scale,
            area.y() * scale,
            area.width() * scale,
            area.height() * scale,
            scale,
        )
    }

    pub fn configure_shelf(window: &gtk::ApplicationWindow, edge: ScreenEdge) {
        set_edge(edge);
        window.set_decorated(false);
        let window = window.clone().upcast::<gtk::Window>();
        window.connect_realize(prepare_shelf);
        window.connect_map(|window| {
            raise(window);
            place_shelf_on_current_monitor(window, current_edge());
            let window = window.clone();
            glib::timeout_add_local_once(SETTLE_DELAY, move || {
                raise(&window);
                place_shelf_on_current_monitor(&window, current_edge());
            });
        });
    }

    /// Stamp the pre-map EWMH properties that make the shelf a floating,
    /// always-on-top utility window rather than a task-switchable one.
    fn prepare_shelf(window: &gtk::Window) {
        let Some(xid) = xid(window) else {
            return;
        };
        with_session(|session| {
            set_atoms(
                session,
                xid,
                session.atoms._NET_WM_WINDOW_TYPE,
                &[session.atoms._NET_WM_WINDOW_TYPE_UTILITY],
            );
            set_atoms(
                session,
                xid,
                session.atoms._NET_WM_STATE,
                &[
                    session.atoms._NET_WM_STATE_ABOVE,
                    session.atoms._NET_WM_STATE_STICKY,
                    session.atoms._NET_WM_STATE_SKIP_TASKBAR,
                    session.atoms._NET_WM_STATE_SKIP_PAGER,
                ],
            );
            let _ = session.connection.flush();
        });
    }

    /// Reassert "above" on a mapped window.
    ///
    /// This runs on every map and after every settle delay: dragging from
    /// another application raises that application, and without this the shelf
    /// would end up behind the window the user is dragging from.
    fn raise(window: &gtk::Window) {
        let Some(xid) = xid(window) else {
            return;
        };
        with_session(|session| {
            send_state(
                session,
                xid,
                session.atoms._NET_WM_STATE_ABOVE,
                session.atoms._NET_WM_STATE_STICKY,
            );
            send_state(
                session,
                xid,
                session.atoms._NET_WM_STATE_SKIP_TASKBAR,
                session.atoms._NET_WM_STATE_SKIP_PAGER,
            );
            let _ = session.connection.flush();
        });
    }

    pub fn place_shelf(window: &gtk::Window, monitor: &gdk::Monitor, edge: ScreenEdge) {
        set_edge(edge);
        let Some(xid) = xid(window) else {
            return;
        };
        let (area_x, area_y, area_width, area_height, scale) = device_area(monitor);
        with_session(|session| {
            // GTK's own width excludes the frame the X server actually gave the
            // window, which would leave the shelf a few pixels off the edge.
            // Ask the server for the real size instead.
            let (width, height) = match session
                .connection
                .get_geometry(xid)
                .map(|cookie| cookie.reply())
            {
                Ok(Ok(geometry)) => (i32::from(geometry.width), i32::from(geometry.height)),
                _ => (
                    SHELF_WIDTH * scale,
                    (area_height - SHELF_VERTICAL_INSET * scale).min(SHELF_MAX_HEIGHT * scale),
                ),
            };
            let x = if edge == ScreenEdge::Right {
                area_x + area_width - width - SHELF_MARGIN * scale
            } else {
                area_x + SHELF_MARGIN * scale
            };
            let y = area_y + (area_height - height.clamp(1, area_height)) / 2;
            let _ = session
                .connection
                .configure_window(xid, &ConfigureWindowAux::new().x(x).y(y));
            let _ = session.connection.flush();
        });
    }

    pub fn place_shelf_on_current_monitor(window: &gtk::Window, edge: ScreenEdge) {
        if let Some(monitor) = current_monitor(window) {
            place_shelf(window, &monitor, edge);
        }
    }

    fn current_monitor(window: &gtk::Window) -> Option<gdk::Monitor> {
        let surface = window.surface()?;
        let display = surface.display();
        display.monitor_at_surface(&surface).or_else(|| {
            display
                .monitors()
                .item(0)
                .and_then(|item| item.downcast::<gdk::Monitor>().ok())
        })
    }

    pub fn configure_edge(
        window: &gtk::Window,
        monitor: &gdk::Monitor,
        strip_size: i32,
        edge: ScreenEdge,
    ) {
        window.set_decorated(false);
        window.set_default_size(
            super::logical_strip_size(strip_size, monitor.scale_factor()),
            1,
        );
        window.connect_realize(prepare_edge);
        let map_monitor = monitor.clone();
        window.connect_map(move |window| {
            raise(window);
            place_edge(window, &map_monitor, strip_size, edge);
            let window = window.clone();
            let monitor = map_monitor.clone();
            glib::timeout_add_local_once(SETTLE_DELAY, move || {
                raise(&window);
                place_edge(&window, &monitor, strip_size, edge);
            });
        });
    }

    /// A dock strip must be typed before it is mapped: Mutter reads
    /// `_NET_WM_WINDOW_TYPE` once, at map time, and ignores later changes.
    fn prepare_edge(window: &gtk::Window) {
        let Some(xid) = xid(window) else {
            return;
        };
        with_session(|session| {
            set_atoms(
                session,
                xid,
                session.atoms._NET_WM_WINDOW_TYPE,
                &[session.atoms._NET_WM_WINDOW_TYPE_DOCK],
            );
            set_atoms(
                session,
                xid,
                session.atoms._NET_WM_STATE,
                &[
                    session.atoms._NET_WM_STATE_ABOVE,
                    session.atoms._NET_WM_STATE_STICKY,
                    session.atoms._NET_WM_STATE_SKIP_TASKBAR,
                    session.atoms._NET_WM_STATE_SKIP_PAGER,
                ],
            );
            let _ = session.connection.flush();
        });
    }

    fn place_edge(window: &gtk::Window, monitor: &gdk::Monitor, strip_size: i32, edge: ScreenEdge) {
        let Some(xid) = xid(window) else {
            return;
        };
        let (area_x, area_y, area_width, area_height, scale) = device_area(monitor);
        let width = strip_size.clamp(3, 16) * scale;
        let x = if edge == ScreenEdge::Right {
            area_x + area_width - width
        } else {
            area_x
        };
        with_session(|session| {
            let _ = session.connection.configure_window(
                xid,
                &ConfigureWindowAux::new()
                    .x(x)
                    .y(area_y)
                    .width(width as u32)
                    .height(area_height as u32),
            );
            let _ = session.connection.flush();
        });
    }
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::mpsc::{self, TryRecvError};
    use std::time::Duration;

    use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;
    use yeet::settings::HotkeyBinding;

    use super::GlobalHotkeyError;

    const TOGGLE_SHORTCUT_ID: &str = "toggle-shelf";

    /// Trace desktop-integration steps when `YEET_DEBUG` is set.
    ///
    /// Portal backends disagree with each other and fail in ways that are
    /// invisible from the UI, so the useful detail has to be reachable from a
    /// user's machine without a debug build.
    fn debug(arguments: std::fmt::Arguments<'_>) {
        use std::sync::OnceLock;

        static ENABLED: OnceLock<bool> = OnceLock::new();
        if *ENABLED.get_or_init(|| std::env::var_os("YEET_DEBUG").is_some()) {
            eprintln!("yeet: {arguments}");
        }
    }

    enum ShortcutEvent {
        Activated,
        Failed(String),
    }

    pub type ErrorSlot = Rc<RefCell<Option<GlobalHotkeyError>>>;

    pub fn install_global_hotkey(shortcut: &str, callback: impl Fn() + 'static) -> ErrorSlot {
        let last_error: ErrorSlot = Rc::new(RefCell::new(None));
        // Keyed off the session rather than the GDK display: the X11 backend
        // runs Yeet's windows through XWayland, but a key grab there only fires
        // while an X11 window has focus, so the portal stays the right
        // mechanism. A pure X11 session has no GlobalShortcuts portal and
        // relies on binding `yeet --toggle` in the window manager instead.
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return last_error;
        }
        let binding = match HotkeyBinding::parse(shortcut) {
            Ok(binding) => binding,
            Err(error) => {
                *last_error.borrow_mut() = Some(GlobalHotkeyError::Invalid(error));
                return last_error;
            }
        };
        // GNOME parses the preferred trigger with GTK's accelerator parser and
        // quietly leaves the shortcut unbound when that fails; KDE follows the
        // XDG specification syntax. Neither accepts the other's spelling.
        let trigger = if super::mutter_session() {
            binding.gtk_accelerator()
        } else {
            binding.portal_trigger()
        };

        // Portal work uses Tokio on a worker thread. Keep the non-Send GTK/UI
        // callback on the main thread and preserve one callback per activation
        // so the UI can continue to detect double presses for clipboard capture.
        let (sender, receiver) = mpsc::channel();
        let error_slot = last_error.clone();
        glib::timeout_add_local(Duration::from_millis(25), move || {
            loop {
                match receiver.try_recv() {
                    Ok(ShortcutEvent::Activated) => {
                        // Proof the shortcut works, whatever the bind reported.
                        error_slot.borrow_mut().take();
                        callback();
                    }
                    Ok(ShortcutEvent::Failed(detail)) => {
                        eprintln!("yeet: global shortcut unconfirmed: {detail}");
                        *error_slot.borrow_mut() = Some(GlobalHotkeyError::Unavailable(detail));
                    }
                    Err(TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(TryRecvError::Disconnected) => return glib::ControlFlow::Break,
                }
            }
        });

        let _ = std::thread::Builder::new()
            .name("yeet-global-shortcuts".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                // A missing portal, an unsupported backend, or a rejected bind
                // is optional integration. In all cases Yeet keeps running and
                // `yeet --toggle` remains available as the compositor fallback.
                runtime.block_on(async move {
                    ensure_app_scope().await;
                    if let Err(error) = run_global_shortcut(&trigger, &sender).await {
                        let _ = sender.send(ShortcutEvent::Failed(error.to_string()));
                    }
                });
            });
        last_error
    }

    /// Give this process a systemd unit whose name carries Yeet's app ID.
    ///
    /// Portals identify a non-sandboxed caller by the unit its process lives
    /// in. A Yeet launched from a terminal inherits the terminal's unit, so
    /// GNOME's backend rejects the bind outright with "An app id is required".
    /// Moving into a transient `app-<app-id>-<pid>.scope` gives the portal the
    /// same view it would get from a desktop-file launch. Every failure here is
    /// non-fatal: the bind is simply attempted as before.
    async fn ensure_app_scope() {
        use ashpd::zbus::zvariant::Value;

        if in_app_scope() {
            return;
        }
        let pid = std::process::id();
        let unit = format!("app-{}-{pid}.scope", crate::APP_ID);
        let result = async {
            let connection = ashpd::zbus::Connection::session().await?;
            let manager = ashpd::zbus::Proxy::new(
                &connection,
                "org.freedesktop.systemd1",
                "/org/freedesktop/systemd1",
                "org.freedesktop.systemd1.Manager",
            )
            .await?;
            let properties: Vec<(&str, Value<'_>)> = vec![
                ("PIDs", Value::from(vec![pid])),
                ("Description", Value::from("Yeet drag-and-drop shelf")),
                ("CollectMode", Value::from("inactive-or-failed")),
            ];
            let auxiliary: Vec<(&str, Vec<(&str, Value<'_>)>)> = Vec::new();
            manager
                .call_method(
                    "StartTransientUnit",
                    &(unit.as_str(), "fail", properties, auxiliary),
                )
                .await?;
            Ok::<_, ashpd::zbus::Error>(())
        }
        .await;
        if let Err(error) = result {
            eprintln!("yeet: could not register a systemd app scope: {error}");
            return;
        }
        // `StartTransientUnit` returns once the job is queued, not once this
        // process has actually been migrated. Connecting to the portal before
        // the migration lands makes it read the old cgroup and reject the
        // session with "An app id is required", so wait for the move to show up.
        for _ in 0..100 {
            if in_app_scope() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        eprintln!("yeet: systemd app scope did not take effect in time");
    }

    /// Report whether this process already runs under a unit named after Yeet.
    ///
    /// A desktop-file launch produces `app-gnome-<app-id>-<pid>.scope` and the
    /// XDG autostart generator produces `app-<app-id>@autostart.service`; both
    /// already satisfy the portal, so neither is disturbed.
    fn in_app_scope() -> bool {
        std::fs::read_to_string("/proc/self/cgroup").is_ok_and(|cgroup| {
            cgroup.lines().any(|line| {
                line.rsplit('/')
                    .next()
                    .is_some_and(|unit| unit.starts_with("app-") && unit.contains(crate::APP_ID))
            })
        })
    }

    async fn run_global_shortcut(
        trigger: &str,
        sender: &mpsc::Sender<ShortcutEvent>,
    ) -> ashpd::Result<()> {
        let portal = GlobalShortcuts::new().await?;
        let session = portal.create_session(Default::default()).await?;
        // Subscribe before binding. A desktop that stored this shortcut on an
        // earlier run keeps delivering activations for it while refusing to
        // bind it a second time, so a failed bind must not cost us the stream.
        //
        // `ListShortcuts` would be the specified way to adopt that stored
        // binding, but GNOME answers it with an empty list and afterwards never
        // answers `BindShortcuts` on the same session at all.
        let mut activated = portal.receive_activated().await?;

        let shortcut = NewShortcut::new(TOGGLE_SHORTCUT_ID, "Show or hide the Yeet shelf")
            .preferred_trigger(trigger);
        let bound = portal
            .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
            .await
            .and_then(|request| request.response());
        match &bound {
            Ok(response) => debug(format_args!("BindShortcuts -> {:?}", response.shortcuts())),
            Err(error) => debug(format_args!("BindShortcuts failed: {error}")),
        }
        let accepted = bound.is_ok_and(|response| {
            response
                .shortcuts()
                .iter()
                .any(|shortcut| shortcut.id() == TOGGLE_SHORTCUT_ID)
        });
        if !accepted {
            // Reported rather than fatal: an activation arriving later clears
            // this, which is what happens when a stored binding is still live.
            let _ = sender.send(ShortcutEvent::Failed(format!(
                "the desktop did not confirm {trigger}; a shortcut stored on an \
                 earlier run may still be live, so try pressing it. \
                 `yeet --toggle` always works."
            )));
        }

        while let Some(event) = activated.next().await {
            if event.shortcut_id() == TOGGLE_SHORTCUT_ID
                && sender.send(ShortcutEvent::Activated).is_err()
            {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::GlobalHotkeyError;
    use gdk_win32::{Win32Display, Win32MessageFilterReturn, Win32Surface};
    use gio::prelude::*;
    use glib::object::Cast;
    use gtk::gdk;
    use gtk::prelude::*;
    use std::sync::atomic::Ordering;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Dwm::{
        DWM_WINDOW_CORNER_PREFERENCE, DWMWA_USE_IMMERSIVE_DARK_MODE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
    };
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, STYLESTRUCT, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SetWindowLongPtrW, SetWindowPos, WM_HOTKEY, WM_NCDESTROY, WM_STYLECHANGING,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    };
    use yeet::settings::{HotkeyBinding, ScreenEdge};

    const HOTKEY_ID: i32 = 0x5945;
    const NATIVE_STYLE_SUBCLASS_ID: usize = 0x5945_4554;

    pub struct GlobalHotkey {
        _filter: Option<gdk_win32::Win32DisplayFilterHandle>,
        current: Option<HotkeyBinding>,
        last_error: Option<GlobalHotkeyError>,
    }

    impl GlobalHotkey {
        pub fn install(shortcut: &str, callback: impl Fn() + 'static) -> Self {
            let Some(display) = gdk::Display::default() else {
                return Self::unavailable("GDK display is not ready");
            };
            let Ok(display) = display.downcast::<Win32Display>() else {
                return Self::unavailable("the active GDK display is not Win32");
            };
            let filter = display.add_filter(move |_, message, _| {
                if message.message == WM_HOTKEY && message.wParam.0 as i32 == HOTKEY_ID {
                    callback();
                    Win32MessageFilterReturn::Remove
                } else {
                    Win32MessageFilterReturn::Continue
                }
            });
            let mut hotkey = Self {
                _filter: Some(filter),
                current: None,
                last_error: None,
            };
            if let Err(error) = hotkey.rebind(shortcut) {
                eprintln!("yeet: {error}");
            }
            hotkey
        }

        fn unavailable(detail: &str) -> Self {
            Self {
                _filter: None,
                current: None,
                last_error: Some(GlobalHotkeyError::Unavailable(detail.to_owned())),
            }
        }

        pub fn registration_error(&self) -> Option<GlobalHotkeyError> {
            self.last_error.clone()
        }

        pub fn rebind(&mut self, shortcut: &str) -> Result<String, GlobalHotkeyError> {
            if self._filter.is_none() {
                let error = self.last_error.clone().unwrap_or_else(|| {
                    GlobalHotkeyError::Unavailable("Win32 message filter is unavailable".to_owned())
                });
                return Err(error);
            }

            let candidate = match HotkeyBinding::parse(shortcut) {
                Ok(candidate) => candidate,
                Err(error) => {
                    let error = GlobalHotkeyError::Invalid(error);
                    if self.current.is_none() {
                        self.last_error = Some(error.clone());
                    }
                    return Err(error);
                }
            };
            if self.current.as_ref() == Some(&candidate) {
                self.last_error = None;
                return Ok(candidate.normalized().to_owned());
            }

            let previous = self.current.take();
            if previous.is_some()
                && let Err(error) = unsafe { UnregisterHotKey(None, HOTKEY_ID) }
            {
                self.current = previous;
                let error = GlobalHotkeyError::Unavailable(format!(
                    "could not release the current shortcut: {error}"
                ));
                self.last_error = None;
                return Err(error);
            }

            match register(&candidate) {
                Ok(()) => {
                    let normalized = candidate.normalized().to_owned();
                    self.current = Some(candidate);
                    self.last_error = None;
                    Ok(normalized)
                }
                Err(register_error) => {
                    let previous_restored = previous
                        .as_ref()
                        .is_some_and(|binding| register(binding).is_ok());
                    self.current = if previous_restored { previous } else { None };
                    let error = GlobalHotkeyError::Conflict {
                        shortcut: candidate.normalized().to_owned(),
                        previous_restored,
                        detail: register_error.to_string(),
                    };
                    self.last_error = (!previous_restored).then_some(error.clone());
                    Err(error)
                }
            }
        }
    }

    impl Drop for GlobalHotkey {
        fn drop(&mut self) {
            if self.current.take().is_some() {
                let _ = unsafe { UnregisterHotKey(None, HOTKEY_ID) };
            }
        }
    }

    fn register(binding: &HotkeyBinding) -> windows::core::Result<()> {
        let modifiers = HOT_KEY_MODIFIERS(binding.modifier_mask()) | MOD_NOREPEAT;
        unsafe { RegisterHotKey(None, HOTKEY_ID, modifiers, binding.virtual_key()) }
    }

    pub fn configure_shelf(window: &gtk::ApplicationWindow, edge: ScreenEdge) {
        set_shelf_edge(edge);
        let window = window.clone().upcast::<gtk::Window>();
        window.connect_realize(move |window| {
            apply_to_current_monitor(window, false, current_shelf_edge())
        });
        // Reassert the native styles every time the hidden shelf is mapped
        // again. GTK can finish adjusting Win32 styles after emitting `map`,
        // so apply once synchronously for placement and again after GDK's
        // native map/configure work has completed.
        window.connect_map(move |window| {
            apply_to_current_monitor(window, false, current_shelf_edge());
            let window = window.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
                apply_to_current_monitor(&window, false, current_shelf_edge())
            });
        });
    }

    pub fn update_shelf_placement(window: &gtk::ApplicationWindow, edge: ScreenEdge) {
        set_shelf_edge(edge);
        apply_to_current_monitor(window.upcast_ref(), false, edge);
    }

    fn set_shelf_edge(edge: ScreenEdge) {
        super::SHELF_EDGE.store(
            if edge == ScreenEdge::Right { 1 } else { 0 },
            Ordering::Relaxed,
        );
    }

    fn current_shelf_edge() -> ScreenEdge {
        if super::SHELF_EDGE.load(Ordering::Relaxed) == 0 {
            ScreenEdge::Left
        } else {
            ScreenEdge::Right
        }
    }

    pub fn configure_window(
        window: &gtk::Window,
        monitor: &gdk::Monitor,
        edge: bool,
        strip_size: i32,
        screen_edge: ScreenEdge,
    ) {
        let realize_monitor = monitor.clone();
        window.connect_realize(move |window| {
            apply(window, &realize_monitor, edge, strip_size, screen_edge)
        });
        let map_monitor = monitor.clone();
        window.connect_map(move |window| {
            apply(window, &map_monitor, edge, strip_size, screen_edge);
            let window = window.clone();
            let monitor = map_monitor.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
                apply(&window, &monitor, edge, strip_size, screen_edge)
            });
        });
    }

    pub fn move_shelf_to_monitor(
        window: &gtk::ApplicationWindow,
        monitor: &gdk::Monitor,
        screen_edge: ScreenEdge,
    ) {
        apply(window.upcast_ref(), monitor, false, 6, screen_edge);
    }

    fn apply_to_current_monitor(window: &gtk::Window, edge: bool, screen_edge: ScreenEdge) {
        let Some(surface) = window.surface() else {
            return;
        };
        let display = surface.display();
        let monitor = display.monitor_at_surface(&surface).or_else(|| {
            display
                .monitors()
                .item(0)
                .and_then(|item| item.downcast::<gdk::Monitor>().ok())
        });
        if let Some(monitor) = monitor {
            apply(window, &monitor, edge, 6, screen_edge);
        }
    }

    fn apply(
        window: &gtk::Window,
        monitor: &gdk::Monitor,
        edge: bool,
        strip_size: i32,
        screen_edge: ScreenEdge,
    ) {
        let Some(surface) = window.surface() else {
            return;
        };
        let Ok(surface) = surface.downcast::<Win32Surface>() else {
            return;
        };
        let hwnd = HWND(surface.handle().0);
        let geometry = monitor.geometry();
        let scale = monitor.scale_factor().max(1);
        let width = if edge {
            strip_size.clamp(3, 16) * scale
        } else {
            yeet::settings::SHELF_WIDTH * scale
        };
        let height = if edge {
            geometry.height()
        } else {
            (geometry.height() - 96 * scale).min(560 * scale)
        };
        let x = if screen_edge == ScreenEdge::Right {
            geometry.x() + geometry.width() - width
        } else {
            geometry.x()
        };
        let y = if edge {
            geometry.y()
        } else {
            geometry.y() + (geometry.height() - height) / 2
        };
        unsafe {
            install_native_style_guard(hwnd, edge);
            let mut style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            style |= (WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0 as isize;
            if edge {
                style |= WS_EX_NOACTIVATE.0 as isize;
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
            apply_native_theme(hwnd);
            if !edge {
                let corners = DWMWCP_ROUND;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    (&corners as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
                    std::mem::size_of_val(&corners) as u32,
                );
            }
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            // `SetWindowPos` generates configure traffic that lets GDK rebuild
            // the extended style. Make the tool-window/no-activate bits the
            // final native write as well as setting them before FRAMECHANGED.
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
        }
    }

    unsafe fn install_native_style_guard(hwnd: HWND, edge: bool) {
        let mut preserved = (WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0;
        if edge {
            preserved |= WS_EX_NOACTIVATE.0;
        }
        if !unsafe {
            SetWindowSubclass(
                hwnd,
                Some(preserve_native_styles),
                NATIVE_STYLE_SUBCLASS_ID,
                preserved as usize,
            )
            .as_bool()
        } {
            eprintln!("yeet: failed to guard native Windows styles");
        }
    }

    unsafe extern "system" fn preserve_native_styles(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        subclass_id: usize,
        preserved: usize,
    ) -> LRESULT {
        if message == WM_STYLECHANGING && wparam.0 as isize == GWL_EXSTYLE.0 as isize {
            let change = lparam.0 as *mut STYLESTRUCT;
            if let Some(change) = unsafe { change.as_mut() } {
                change.styleNew |= preserved as u32;
            }
        } else if message == WM_NCDESTROY {
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(preserve_native_styles), subclass_id);
            }
        }
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    pub fn refresh_window_theme(window: &gtk::Window) {
        let Some(surface) = window.surface() else {
            return;
        };
        let Ok(surface) = surface.downcast::<Win32Surface>() else {
            return;
        };
        unsafe { apply_native_theme(HWND(surface.handle().0)) };
    }

    unsafe fn apply_native_theme(hwnd: HWND) {
        let dark: i32 = i32::from(prefers_dark());
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark as *const i32).cast(),
                std::mem::size_of::<i32>() as u32,
            )
        };
    }

    fn prefers_dark() -> bool {
        match super::THEME_OVERRIDE.load(Ordering::Relaxed) {
            1 => false,
            2 => true,
            _ => std::process::Command::new("reg")
                .args([
                    "query",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                    "/v",
                    "AppsUseLightTheme",
                ])
                .output()
                .is_ok_and(|output| {
                    String::from_utf8_lossy(&output.stdout)
                        .split_whitespace()
                        .last()
                        .is_some_and(|value| value == "0x0")
                }),
        }
    }
}
