use gtk::gdk;

#[cfg(not(target_os = "linux"))]
use gtk::prelude::GtkWindowExt;

#[cfg(target_os = "linux")]
pub fn layer_shell_supported() -> bool {
    use glib::prelude::ObjectExt;

    if std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktop| desktop.to_ascii_lowercase().contains("gnome"))
    {
        return false;
    }
    let is_wayland = gdk::Display::default()
        .is_some_and(|display| display.type_().name() == "GdkWaylandDisplay");
    is_wayland && gtk4_layer_shell::is_supported()
}

#[cfg(not(target_os = "windows"))]
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
pub fn configure_shelf(window: &gtk::ApplicationWindow) {
    use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    if !layer_shell_supported() {
        return;
    }
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("yeet-shelf"));
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_margin(Edge::Right, 8);
    window.set_margin(Edge::Top, 48);
    window.set_margin(Edge::Bottom, 48);
    window.set_exclusive_zone(0);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
}

#[cfg(target_os = "windows")]
pub fn configure_shelf(window: &gtk::ApplicationWindow) {
    window.set_decorated(false);
    windows_impl::configure_shelf(window);
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn configure_shelf(window: &gtk::ApplicationWindow) {
    window.set_decorated(false);
}

#[cfg(target_os = "linux")]
pub fn configure_edge(window: &gtk::Window, monitor: &gdk::Monitor, _strip_size: i32) {
    use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    if !layer_shell_supported() {
        return;
    }
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("yeet-edge-strip"));
    window.set_monitor(Some(monitor));
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_exclusive_zone(0);
    window.set_keyboard_mode(KeyboardMode::None);
}

#[cfg(target_os = "linux")]
pub fn set_shelf_monitor(window: &gtk::ApplicationWindow, monitor: &gdk::Monitor) {
    use gtk4_layer_shell::LayerShell;

    if layer_shell_supported() {
        window.set_monitor(Some(monitor));
    }
}

#[cfg(target_os = "windows")]
pub fn configure_edge(window: &gtk::Window, monitor: &gdk::Monitor, strip_size: i32) {
    windows_impl::configure_window(window, monitor, true, strip_size);
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn configure_edge(window: &gtk::Window, _monitor: &gdk::Monitor, _strip_size: i32) {
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
pub fn set_shelf_monitor(window: &gtk::ApplicationWindow, monitor: &gdk::Monitor) {
    windows_impl::move_shelf_to_monitor(window, monitor);
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
pub fn set_shelf_monitor(_window: &gtk::ApplicationWindow, _monitor: &gdk::Monitor) {}

#[cfg(target_os = "windows")]
mod windows_impl {
    use gdk_win32::{Win32Display, Win32MessageFilterReturn, Win32Surface};
    use gio::prelude::*;
    use glib::object::Cast;
    use gtk::gdk;
    use gtk::prelude::*;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
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

    pub fn configure_shelf(window: &gtk::ApplicationWindow) {
        let window = window.clone().upcast::<gtk::Window>();
        window.connect_realize(|window| apply_to_current_monitor(window, false));
        // Reassert HWND_TOPMOST every time the hidden shelf is mapped again.
        window.connect_map(|window| apply_to_current_monitor(window, false));
    }

    pub fn configure_window(
        window: &gtk::Window,
        monitor: &gdk::Monitor,
        edge: bool,
        strip_size: i32,
    ) {
        let realize_monitor = monitor.clone();
        window.connect_realize(move |window| apply(window, &realize_monitor, edge, strip_size));
        let map_monitor = monitor.clone();
        window.connect_map(move |window| apply(window, &map_monitor, edge, strip_size));
    }

    pub fn move_shelf_to_monitor(window: &gtk::ApplicationWindow, monitor: &gdk::Monitor) {
        apply(window.upcast_ref(), monitor, false, 6);
    }

    fn apply_to_current_monitor(window: &gtk::Window, edge: bool) {
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
            apply(window, &monitor, edge, 6);
        }
    }

    fn apply(window: &gtk::Window, monitor: &gdk::Monitor, edge: bool, strip_size: i32) {
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
        let x = geometry.x() + geometry.width() - width;
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
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark as *const i32).cast(),
                std::mem::size_of::<i32>() as u32,
            );
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
}
