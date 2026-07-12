use gtk::gdk;
use wayland_yeet::settings::{ScreenEdge, Theme};

#[cfg(target_os = "windows")]
static THEME_OVERRIDE: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(0);

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

#[cfg(not(target_os = "linux"))]
use gtk::prelude::GtkWindowExt;

#[cfg(target_os = "linux")]
pub fn layer_shell_supported() -> bool {
    if std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktop| desktop.to_ascii_lowercase().contains("gnome"))
    {
        return false;
    }
    wayland_display_available() && gtk4_layer_shell::is_supported()
}

#[cfg(target_os = "linux")]
pub fn install_global_hotkey(callback: impl Fn() + 'static) {
    linux_impl::install_global_hotkey(callback);
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn install_global_hotkey(_callback: impl Fn() + 'static) {}

#[cfg(target_os = "windows")]
pub fn install_global_hotkey(callback: impl Fn() + 'static) {
    windows_impl::install_global_hotkey(callback);
}

#[cfg(not(target_os = "linux"))]
pub fn layer_shell_supported() -> bool {
    false
}

#[cfg(target_os = "linux")]
pub fn configure_shelf(window: &gtk::ApplicationWindow, edge: ScreenEdge) {
    use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    if !layer_shell_supported() {
        return;
    }
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("yeet-shelf"));
    window.set_anchor(Edge::Right, edge == ScreenEdge::Right);
    window.set_anchor(Edge::Left, edge == ScreenEdge::Left);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_margin(Edge::Right, if edge == ScreenEdge::Right { 8 } else { 0 });
    window.set_margin(Edge::Left, if edge == ScreenEdge::Left { 8 } else { 0 });
    window.set_margin(Edge::Top, 48);
    window.set_margin(Edge::Bottom, 48);
    window.set_exclusive_zone(0);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
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
pub fn configure_edge(
    window: &gtk::Window,
    monitor: &gdk::Monitor,
    _strip_size: i32,
    edge: ScreenEdge,
) {
    use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    if !layer_shell_supported() {
        return;
    }
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("yeet-edge-strip"));
    window.set_monitor(Some(monitor));
    window.set_anchor(Edge::Right, edge == ScreenEdge::Right);
    window.set_anchor(Edge::Left, edge == ScreenEdge::Left);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_exclusive_zone(0);
    window.set_keyboard_mode(KeyboardMode::None);
}

#[cfg(target_os = "linux")]
pub fn set_shelf_monitor(
    window: &gtk::ApplicationWindow,
    monitor: &gdk::Monitor,
    _edge: ScreenEdge,
) {
    use gtk4_layer_shell::LayerShell;

    if layer_shell_supported() {
        window.set_monitor(Some(monitor));
    }
}

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

#[cfg(target_os = "linux")]
mod linux_impl {
    use std::sync::mpsc::{self, TryRecvError};
    use std::time::Duration;

    use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    const TOGGLE_SHORTCUT_ID: &str = "toggle-shelf";

    pub fn install_global_hotkey(callback: impl Fn() + 'static) {
        if !super::wayland_display_available() {
            return;
        }

        // Portal work uses Tokio on a worker thread. Keep the non-Send GTK/UI
        // callback on the main thread and preserve one callback per activation
        // so the UI can continue to detect double presses for clipboard capture.
        let (sender, receiver) = mpsc::channel();
        glib::timeout_add_local(Duration::from_millis(25), move || {
            loop {
                match receiver.try_recv() {
                    Ok(()) => callback(),
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
                let _ = runtime.block_on(run_global_shortcut(sender));
            });
    }

    async fn run_global_shortcut(sender: mpsc::Sender<()>) -> ashpd::Result<()> {
        let portal = GlobalShortcuts::new().await?;
        let session = portal.create_session(Default::default()).await?;
        let shortcut = NewShortcut::new(TOGGLE_SHORTCUT_ID, "Show or hide the Yeet shelf")
            .preferred_trigger("CTRL+ALT+Y");
        let request = portal
            .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
            .await?;
        let bound = request.response()?;
        if !bound
            .shortcuts()
            .iter()
            .any(|shortcut| shortcut.id() == TOGGLE_SHORTCUT_ID)
        {
            return Ok(());
        }

        let mut activated = portal.receive_activated().await?;
        while let Some(event) = activated.next().await {
            if event.shortcut_id() == TOGGLE_SHORTCUT_ID && sender.send(()).is_err() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use gdk_win32::{Win32Display, Win32MessageFilterReturn, Win32Surface};
    use gio::prelude::*;
    use glib::object::Cast;
    use gtk::gdk;
    use gtk::prelude::*;
    use std::sync::atomic::Ordering;
    use wayland_yeet::settings::ScreenEdge;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DwmSetWindowAttribute,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SetWindowLongPtrW, SetWindowPos, WM_HOTKEY, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST,
    };

    const HOTKEY_ID: i32 = 0x5945;

    pub fn install_global_hotkey(callback: impl Fn() + 'static) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let Ok(display) = display.downcast::<Win32Display>() else {
            return;
        };
        if let Err(error) = unsafe {
            RegisterHotKey(
                None,
                HOTKEY_ID,
                MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                b'Y' as u32,
            )
        } {
            eprintln!("yeet: Ctrl+Alt+Y is unavailable: {error}");
            return;
        }
        let filter = display.add_filter(move |_, message, _| {
            if message.message == WM_HOTKEY && message.wParam.0 as i32 == HOTKEY_ID {
                callback();
                Win32MessageFilterReturn::Remove
            } else {
                Win32MessageFilterReturn::Continue
            }
        });
        Box::leak(Box::new(filter));
    }

    pub fn configure_shelf(window: &gtk::ApplicationWindow, edge: ScreenEdge) {
        let window = window.clone().upcast::<gtk::Window>();
        window.connect_realize(move |window| apply_to_current_monitor(window, false, edge));
        // Reassert HWND_TOPMOST every time the hidden shelf is mapped again.
        window.connect_map(move |window| apply_to_current_monitor(window, false, edge));
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
        window
            .connect_map(move |window| apply(window, &map_monitor, edge, strip_size, screen_edge));
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
            300 * scale
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
            let mut style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            style |= (WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0 as isize;
            if edge {
                style |= WS_EX_NOACTIVATE.0 as isize;
            }
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
            let dark: i32 = i32::from(prefers_dark());
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark as *const i32).cast(),
                std::mem::size_of::<i32>() as u32,
            );
            if !edge {
                let corners = DWMWCP_ROUND;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    (&corners as *const _).cast(),
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
        }
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
