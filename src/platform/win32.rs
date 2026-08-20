//! Win32 window behaviour: topmost styles, placement and the hotkey.
//!
//! Unlike Wayland, Windows lets an application place and raise its own windows,
//! so the shelf and edge strips are positioned directly with `SetWindowPos` and
//! held above other windows with `WS_EX_TOPMOST`. GTK re-derives the extended
//! style after its own configure work, so the native bits are reasserted and
//! guarded with an `HWND` subclass.

use super::GlobalHotkeyError;
use gdk_win32::{Win32Display, Win32MessageFilterReturn, Win32Surface};
use gio::prelude::*;
use glib::object::Cast;
use gtk::gdk;
use gtk::prelude::*;
use std::sync::atomic::Ordering;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY, GWL_EXSTYLE, GetClassNameW,
    GetWindowLongPtrW, HWND_TOPMOST, OBJID_WINDOW, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SetWindowLongPtrW, SetWindowPos, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_HOTKEY,
    WM_NCDESTROY, WM_STYLECHANGING, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
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
    // A strip is always anchored; only the shelf follows a position the user
    // dragged it to, clamped so a stale position cannot land it off-screen.
    let manual = (!edge).then(super::manual_shelf_position).flatten();
    let (x, y) = match manual {
        Some((x, y)) => (
            x.clamp(geometry.x(), geometry.x() + geometry.width() - width),
            y.clamp(geometry.y(), geometry.y() + geometry.height() - height),
        ),
        None => {
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
            (x, y)
        }
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

/// Where the shelf currently sits, in virtual-screen coordinates.
pub fn current_position(window: &gtk::Window) -> Option<(i32, i32)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    let surface = window.surface()?;
    let surface = surface.downcast::<Win32Surface>().ok()?;
    let hwnd = HWND(surface.handle().0);
    let mut rectangle = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rectangle) }.ok()?;
    Some((rectangle.left, rectangle.top))
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

/// Whether the Windows app theme is currently dark.
///
/// Read directly from the registry. This runs on every realize and map of the
/// shelf and of every edge strip, so the `reg.exe` query it replaces spawned a
/// process — and flashed a console window — several times per launch.
fn prefers_dark() -> bool {
    const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

    match super::THEME_OVERRIDE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        // Absent on a system whose theme was never changed, which is light.
        _ => super::registry::current_user_dword(PERSONALIZE, "AppsUseLightTheme") == Some(0),
    }
}

/// The class of the window the shell's drag helper creates to carry the drag
/// image. Its life is exactly the life of an OLE drag, and its creation and
/// destruction are both reported by an accessibility event hook — which is
/// the closest thing Windows offers to the global drag notification macOS
/// gives Yoink.
const DRAG_IMAGE_CLASS: &str = "SysDragImage";

/// What the hook procedure calls once it has decided a drag started or ended.
type DragObserver = std::rc::Rc<dyn Fn(super::DragPhase)>;

thread_local! {
    /// Where the hook procedure finds the UI. A `WINEVENTPROC` is a plain
    /// function pointer with no user data, and an out-of-context hook is
    /// dispatched on the thread that installed it, so the main thread's own
    /// storage is both the only option and the correct one.
    ///
    /// Reference counted so the hook can let go of the cell before calling
    /// out: window events keep arriving while the shelf is being revealed, and
    /// a borrow held across that call would turn a re-entrant event into a
    /// panic inside an FFI callback.
    static DRAG_OBSERVER: std::cell::RefCell<Option<DragObserver>> =
        const { std::cell::RefCell::new(None) };
    /// The drag image window currently on screen, so an unrelated window
    /// closing cannot be mistaken for the end of a drag.
    static DRAG_IMAGE: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

pub struct DragWatch {
    hook: HWINEVENTHOOK,
}

impl Drop for DragWatch {
    fn drop(&mut self) {
        let _ = unsafe { UnhookWinEvent(self.hook) };
        DRAG_OBSERVER.with(|observer| observer.borrow_mut().take());
        DRAG_IMAGE.with(|hwnd| hwnd.set(0));
    }
}

pub fn watch_global_drags(callback: impl Fn(super::DragPhase) + 'static) -> Option<DragWatch> {
    // Out-of-context so nothing of Yeet is injected into the applications
    // being watched, and skipping our own process so dragging an item off the
    // shelf never looks like a reason to reveal the shelf.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_DESTROY,
            None,
            Some(on_window_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    if hook.is_invalid() {
        eprintln!("yeet: could not watch for drags; the edge strip still reveals the shelf");
        return None;
    }
    DRAG_OBSERVER.with(|observer| *observer.borrow_mut() = Some(std::rc::Rc::new(callback)));
    DRAG_IMAGE.with(|hwnd| hwnd.set(0));
    Some(DragWatch { hook })
}

unsafe extern "system" fn on_window_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // Every control, menu item and list row in the session comes through here.
    // Rejecting everything that is not a top-level window keeps the work to
    // two integer comparisons for all of them.
    if id_object != OBJID_WINDOW.0 || id_child != CHILDID_SELF as i32 || hwnd.is_invalid() {
        return;
    }
    let phase = match event {
        EVENT_OBJECT_CREATE => {
            if !is_drag_image(hwnd) || DRAG_IMAGE.with(|current| current.get()) != 0 {
                return;
            }
            DRAG_IMAGE.with(|current| current.set(hwnd.0 as isize));
            super::DragPhase::Begin
        }
        EVENT_OBJECT_DESTROY => {
            if DRAG_IMAGE.with(|current| current.get()) != hwnd.0 as isize {
                return;
            }
            DRAG_IMAGE.with(|current| current.set(0));
            super::DragPhase::End
        }
        _ => return,
    };
    let observer = DRAG_OBSERVER.with(|observer| observer.borrow().clone());
    if let Some(observer) = observer {
        observer(phase);
    }
}

/// Whether a window is the shell's drag image.
///
/// Asked only of freshly created top-level windows, and only until one is
/// found, so the class-name round trip is rare. A destroyed window is matched
/// by handle instead: its class is no longer readable by the time the event
/// arrives.
fn is_drag_image(hwnd: HWND) -> bool {
    let mut class = [0u16; 64];
    let length = unsafe { GetClassNameW(hwnd, &mut class) };
    if length <= 0 {
        return false;
    }
    class[..length as usize]
        .iter()
        .copied()
        .eq(DRAG_IMAGE_CLASS.encode_utf16())
}
