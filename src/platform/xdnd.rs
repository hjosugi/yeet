//! Global drag detection on X11 and XWayland.
//!
//! Yoink reveals its shelf the moment a drag starts anywhere on screen, and
//! [`crate::platform`]'s edge strip only approximates that: the user has to
//! aim at the strip first. On X11 the real thing is available through public
//! protocol. XDND requires a drag source to take ownership of the
//! `XdndSelection` selection before it moves the pointer, so the XFIXES
//! extension's selection-owner notification *is* a drag-start notification —
//! delivered as an event, with no polling and no pointer hook.
//!
//! Two things this deliberately does not do. It never looks at the drag's
//! contents: the owner window id and the fact that ownership changed are all
//! that leave this module, so Yeet learns that *a* drag exists and nothing
//! about what is being dragged. And it never grabs anything, so a drag that
//! ignores Yeet is unaffected by the watch.
//!
//! Ending is the harder half. XDND leaves selection ownership with the source
//! after the drop so the target can still fetch the data, so there is no
//! matching "released" event to wait for. The pointer button is the reliable
//! signal instead, and it is only sampled while a drag this module has already
//! announced is still in flight — an idle Yeet makes no X requests at all.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::Sender;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as XfixesConnectionExt, SelectionEventMask};
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};
use x11rb::rust_connection::RustConnection;

use super::DragPhase;

/// How often the pointer is sampled while a drag is in flight.
///
/// Short enough that the shelf does not linger after a cancelled drag, long
/// enough that a slow drag across a large desktop costs a handful of round
/// trips rather than hundreds.
const DRAG_POLL_INTERVAL: Duration = Duration::from_millis(120);

/// A drag nobody finishes must not keep the sampler alive forever. A source
/// that dies mid-drag, or a pointer grab Yeet never sees released, ends the
/// drag here instead.
const DRAG_MAX_DURATION: Duration = Duration::from_secs(120);

/// Whether the pointer still holds any button, which is what "a drag is still
/// in flight" means to every drag source there is.
fn dragging(mask: KeyButMask) -> bool {
    let buttons = KeyButMask::BUTTON1
        | KeyButMask::BUTTON2
        | KeyButMask::BUTTON3
        | KeyButMask::BUTTON4
        | KeyButMask::BUTTON5;
    u16::from(mask) & u16::from(buttons) != 0
}

/// Report whether this session can tell Yeet that a drag started.
///
/// Only the environment is inspected, so this is answerable before GTK opens a
/// display and cheap enough to call from the settings dialog. A session with
/// no `DISPLAY` has no XDND at all; one with `DISPLAY` may still turn out to
/// lack XFIXES, which [`watch`] reports by returning `None`.
pub fn available() -> bool {
    std::env::var_os("DISPLAY").is_some()
}

pub struct Watch {
    stopped: Arc<AtomicBool>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        // The watcher thread is parked in `wait_for_event`, so it notices the
        // flag at the next drag rather than immediately. That is one wasted
        // wakeup at worst, and it costs no timer to arrange.
        self.stopped.store(true, Ordering::Relaxed);
    }
}

/// Start watching for drags, reporting each phase over `sender`.
///
/// Returns `None` when the X server is unreachable or does not implement
/// XFIXES, which is the caller's cue to fall back to the edge strip alone.
pub fn watch(sender: Sender<DragPhase>) -> Option<Watch> {
    watch_selection(XDND_SELECTION, sender)
}

/// [`watch`], with the selection named rather than assumed.
///
/// Only the tests pass anything else: they use a selection of their own so
/// that exercising this against a live X server cannot disturb a real drag.
fn watch_selection(selection: &str, sender: Sender<DragPhase>) -> Option<Watch> {
    if !available() {
        return None;
    }
    let session = Session::open(selection)?;
    let stopped = Arc::new(AtomicBool::new(false));
    let thread_stopped = stopped.clone();
    thread::Builder::new()
        .name("yeet-drag-watch".into())
        .spawn(move || session.run(&sender, &thread_stopped))
        .ok()?;
    Some(Watch { stopped })
}

/// The selection every XDND drag source owns for the length of its drag.
const XDND_SELECTION: &str = "XdndSelection";

struct Session {
    connection: RustConnection,
    root: u32,
    selection: u32,
}

impl Session {
    fn open(selection: &str) -> Option<Self> {
        let (connection, screen) = x11rb::connect(None)
            .inspect_err(|error| eprintln!("yeet: drag watch unavailable: {error}"))
            .ok()?;
        let root = connection.setup().roots.get(screen)?.root;
        // XFIXES refuses every other request until the version is negotiated.
        // Selection notifications have been in the extension since version 1.
        if let Err(error) = connection
            .xfixes_query_version(5, 0)
            .map_err(|error| error.to_string())
            .and_then(|cookie| cookie.reply().map_err(|error| error.to_string()))
        {
            eprintln!("yeet: XFIXES unavailable, drags will only be seen at the edge: {error}");
            return None;
        }
        let selection = connection
            .intern_atom(false, selection.as_bytes())
            .ok()?
            .reply()
            .ok()?
            .atom;
        connection
            .xfixes_select_selection_input(
                root,
                selection,
                SelectionEventMask::SET_SELECTION_OWNER
                    | SelectionEventMask::SELECTION_WINDOW_DESTROY
                    | SelectionEventMask::SELECTION_CLIENT_CLOSE,
            )
            .ok()?
            .check()
            .inspect_err(|error| eprintln!("yeet: drag watch was refused: {error}"))
            .ok()?;
        Some(Self {
            connection,
            root,
            selection,
        })
    }

    fn run(&self, sender: &Sender<DragPhase>, stopped: &AtomicBool) {
        while let Ok(event) = self.connection.wait_for_event() {
            if stopped.load(Ordering::Relaxed) || sender.is_closed() {
                return;
            }
            if !self.starts_a_drag(&event) {
                continue;
            }
            if sender.try_send(DragPhase::Begin).is_err() {
                return;
            }
            self.wait_for_release(sender, stopped);
            if sender.try_send(DragPhase::End).is_err() {
                return;
            }
        }
    }

    fn starts_a_drag(&self, event: &Event) -> bool {
        // A source that takes the selection is starting a drag; one that drops
        // it or disappears is ending one, which the pointer already told us.
        matches!(
            event,
            Event::XfixesSelectionNotify(notify)
                if notify.selection == self.selection && notify.owner != x11rb::NONE
        )
    }

    /// Block until every pointer button is up, the watch is dropped, or the
    /// drag has run long enough to be considered lost.
    fn wait_for_release(&self, sender: &Sender<DragPhase>, stopped: &AtomicBool) {
        let deadline = Instant::now() + DRAG_MAX_DURATION;
        loop {
            thread::sleep(DRAG_POLL_INTERVAL);
            if stopped.load(Ordering::Relaxed) || sender.is_closed() || Instant::now() >= deadline {
                return;
            }
            // Selection traffic keeps arriving during the drag; dropping it
            // here keeps the queue from growing until the drag is over.
            while matches!(self.connection.poll_for_event(), Ok(Some(_))) {}
            let Ok(Ok(pointer)) = self
                .connection
                .query_pointer(self.root)
                .map(|cookie| cookie.reply())
            else {
                return;
            };
            if !dragging(pointer.mask) {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};

    /// A selection no real application owns, so this exercises the live X
    /// server without touching `XdndSelection` and the drags that use it.
    const TEST_SELECTION: &str = "_YEET_DRAG_WATCH_TEST";

    /// How long a round trip through the X server, the watcher thread and the
    /// channel is allowed to take before the test calls it a failure.
    const DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);

    /// Take ownership of `TEST_SELECTION`, which is exactly what a drag source
    /// does to `XdndSelection` when a drag begins.
    fn claim_test_selection() -> Option<(RustConnection, u32)> {
        let (connection, screen) = x11rb::connect(None).ok()?;
        let root = connection.setup().roots.get(screen)?.root;
        let window = connection.generate_id().ok()?;
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .ok()?
            .check()
            .ok()?;
        let selection = connection
            .intern_atom(false, TEST_SELECTION.as_bytes())
            .ok()?
            .reply()
            .ok()?
            .atom;
        connection
            .set_selection_owner(window, selection, x11rb::CURRENT_TIME)
            .ok()?
            .check()
            .ok()?;
        connection.flush().ok()?;
        Some((connection, window))
    }

    /// The whole mechanism against a real X server: a new selection owner is
    /// reported as a drag beginning, and — with no pointer button held, which
    /// is the state of an unattended test machine — as ending straight after.
    ///
    /// Skipped where there is no X server to ask. That covers a Wayland-only
    /// session, where this backend is unavailable in exactly the same way.
    #[test]
    fn a_new_selection_owner_is_reported_as_a_drag() {
        if !available() {
            return;
        }
        let (sender, receiver) = async_channel::unbounded();
        let Some(_watch) = watch_selection(TEST_SELECTION, sender) else {
            return;
        };
        let Some((_connection, _window)) = claim_test_selection() else {
            return;
        };

        let deadline = Instant::now() + DELIVERY_TIMEOUT;
        let mut phases = Vec::new();
        while Instant::now() < deadline && phases.len() < 2 {
            match receiver.try_recv() {
                Ok(phase) => phases.push(phase),
                Err(_) => thread::sleep(Duration::from_millis(20)),
            }
        }
        assert_eq!(
            phases,
            [DragPhase::Begin, DragPhase::End],
            "taking the selection should open and then close one drag"
        );
    }

    /// Dropping the watch has to stop the reveals. The thread is parked on the
    /// X connection, so it stops at the next event rather than instantly, and
    /// what matters is that nothing reaches the receiver afterwards.
    #[test]
    fn a_dropped_watch_delivers_nothing_further() {
        if !available() {
            return;
        }
        let (sender, receiver) = async_channel::unbounded();
        let Some(watch) = watch_selection(TEST_SELECTION, sender) else {
            return;
        };
        drop(watch);
        let Some((_connection, _window)) = claim_test_selection() else {
            return;
        };
        thread::sleep(DRAG_POLL_INTERVAL * 4);
        assert!(receiver.try_recv().is_err(), "a dropped watch stays quiet");
    }
}
