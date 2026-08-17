//! X11 (and XWayland) shelf placement.
//!
//! Mutter refuses Wayland clients any say over stacking or position, but it is
//! a conforming EWMH window manager for X11 clients: it honours
//! `_NET_WM_WINDOW_TYPE_DOCK` for the edge strips and `_NET_WM_STATE_ABOVE`
//! for the shelf. Running Yeet's GTK windows through XWayland is therefore the
//! only way to get Yoink's "never buried" guarantee on a stock GNOME session.
//!
//! GTK 4 exposes no window positioning at all, so geometry is applied straight
//! to the X server over a second connection. Owning that connection separately
//! from GDK's keeps this independent of GDK's own X11 bookkeeping; every call
//! degrades to a no-op when the connection or the surface is missing.

use super::ScreenEdge;
use gtk::gdk;
use gtk::prelude::*;
use std::cell::OnceCell;
use std::sync::atomic::{AtomicI8, Ordering};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt, EventMask, PropMode, Window,
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
        let (x, y) = match super::manual_shelf_position() {
            // Clamped so a position saved on a monitor that is no longer
            // attached cannot strand the shelf off-screen.
            Some((x, y)) => (
                x.clamp(area_x, (area_x + area_width - width).max(area_x)),
                y.clamp(area_y, (area_y + area_height - height).max(area_y)),
            ),
            None => {
                let x = if edge == ScreenEdge::Right {
                    area_x + area_width - width - SHELF_MARGIN * scale
                } else {
                    area_x + SHELF_MARGIN * scale
                };
                (x, area_y + (area_height - height.clamp(1, area_height)) / 2)
            }
        };
        let _ = session
            .connection
            .configure_window(xid, &ConfigureWindowAux::new().x(x).y(y));
        let _ = session.connection.flush();
    });
}

/// Where the shelf currently sits, in root-window coordinates.
///
/// Read with `translate_coordinates` rather than `get_geometry`, whose x and y
/// are relative to the parent — which is the window manager's frame once the
/// window is reparented, not the screen.
pub fn current_position(window: &gtk::Window) -> Option<(i32, i32)> {
    let xid = xid(window)?;
    with_session(|session| {
        let translated = session
            .connection
            .translate_coordinates(xid, session.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some((i32::from(translated.dst_x), i32::from(translated.dst_y)))
    })
    .flatten()
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
