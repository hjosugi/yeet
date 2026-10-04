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
//! matching "released" event to wait for. Which signal stands in for it
//! depends on who is dragging:
//!
//! - An X11 client drags under a pointer grab the X server can see, so the
//!   pointer button is the reliable signal.
//! - A Wayland-native drag on Mutter is mirrored into `XdndSelection` by the
//!   compositor, but XWayland never sees its button or its pointer moving. The
//!   compositor answers `TARGETS` on the selection for exactly as long as it
//!   still holds the drag, so that answer is the signal. The request is served
//!   from the compositor's own bookkeeping and never reaches the application
//!   being dragged from, and the list it returns is discarded unread.
//!
//! Either is only sampled while a drag this module has already announced is
//! still in flight — an idle Yeet makes no X requests at all.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::Sender;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as XfixesConnectionExt, SelectionEventMask};
use x11rb::protocol::xproto::{ConnectionExt, CreateWindowAux, KeyButMask, WindowClass};
use x11rb::rust_connection::RustConnection;

use super::DragPhase;

/// How often a drag in flight is checked for its end.
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
/// in flight" means to every X11 drag source there is.
fn dragging(mask: KeyButMask) -> bool {
    let buttons = KeyButMask::BUTTON1
        | KeyButMask::BUTTON2
        | KeyButMask::BUTTON3
        | KeyButMask::BUTTON4
        | KeyButMask::BUTTON5;
    u16::from(mask) & u16::from(buttons) != 0
}

/// How the end of one drag is recognised, decided once as it starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tracking {
    /// The X server sees the button driving the drag, so the drag is over once
    /// no button is held.
    Pointer,
    /// No button is held as far as X can tell: the Wayland compositor owns the
    /// drag and only mirrors it into `XdndSelection`. XWayland sees neither the
    /// button nor the pointer for its whole length, so the drag lasts until the
    /// selection owner stops offering anything.
    Compositor,
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
    /// An unmapped window that receives the answers to the drag probes.
    requestor: u32,
    targets: u32,
    /// Where a probe's answer is delivered; deleted again unread.
    property: u32,
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
        let selection = intern(&connection, selection)?;
        let targets = intern(&connection, "TARGETS")?;
        let property = intern(&connection, "_YEET_DRAG_PROBE")?;
        let requestor = connection.generate_id().ok()?;
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                requestor,
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
            requestor,
            targets,
            property,
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
        // it or disappears is ending one, which `wait_for_release` has already
        // acted on.
        matches!(
            event,
            Event::XfixesSelectionNotify(notify)
                if notify.selection == self.selection && notify.owner != x11rb::NONE
        )
    }

    /// Block until the drag is over, the watch is dropped, or the drag has run
    /// long enough to be considered lost.
    fn wait_for_release(&self, sender: &Sender<DragPhase>, stopped: &AtomicBool) {
        let deadline = Instant::now() + DRAG_MAX_DURATION;
        let tracking = match self.buttons_held() {
            Some(true) => Tracking::Pointer,
            Some(false) => Tracking::Compositor,
            None => return,
        };
        // A probe is outstanding until its answer arrives; asking again before
        // then would only queue up duplicate answers.
        let mut probing = false;
        loop {
            if tracking == Tracking::Compositor && !probing {
                if !self.probe() {
                    return;
                }
                probing = true;
            }
            thread::sleep(DRAG_POLL_INTERVAL);
            if stopped.load(Ordering::Relaxed) || sender.is_closed() || Instant::now() >= deadline {
                return;
            }
            while let Ok(Some(event)) = self.connection.poll_for_event() {
                match event {
                    Event::SelectionNotify(notify) if notify.requestor == self.requestor => {
                        probing = false;
                        // A refusal means the owner has no drag left to offer.
                        if notify.property == x11rb::NONE {
                            return;
                        }
                        let _ = self
                            .connection
                            .delete_property(self.requestor, notify.property);
                    }
                    Event::XfixesSelectionNotify(notify)
                        if notify.selection == self.selection && notify.owner == x11rb::NONE =>
                    {
                        return;
                    }
                    _ => {}
                }
            }
            if tracking == Tracking::Pointer && self.buttons_held() != Some(true) {
                return;
            }
        }
    }

    /// Whether the X server sees any pointer button held.
    fn buttons_held(&self) -> Option<bool> {
        let reply = self
            .connection
            .query_pointer(self.root)
            .ok()?
            .reply()
            .ok()?;
        Some(dragging(reply.mask))
    }

    /// Ask the selection owner which types it offers, answered on the
    /// requestor window. Returns whether the request could be sent.
    fn probe(&self) -> bool {
        self.connection
            .convert_selection(
                self.requestor,
                self.selection,
                self.targets,
                self.property,
                x11rb::CURRENT_TIME,
            )
            .is_ok()
            && self.connection.flush().is_ok()
    }
}

fn intern(connection: &RustConnection, name: &str) -> Option<u32> {
    Some(
        connection
            .intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()?
            .atom,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto::{
        AtomEnum, EventMask, PropMode, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent,
    };
    use x11rb::wrapper::ConnectionExt as _;

    /// Selections no real application owns, so these exercise the live X
    /// server without touching `XdndSelection` and the drags that use it. Each
    /// test has its own: tests run in parallel, and one test's owner taking a
    /// shared selection would end another test's drag.
    const BEGIN_END_SELECTION: &str = "_YEET_DRAG_WATCH_TEST_BEGIN_END";
    const OFFERED_SELECTION: &str = "_YEET_DRAG_WATCH_TEST_OFFERED";
    const DROPPED_SELECTION: &str = "_YEET_DRAG_WATCH_TEST_DROPPED";

    /// How long a round trip through the X server, the watcher thread and the
    /// channel is allowed to take before the test calls it a failure.
    const DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);

    /// Take ownership of the selection `name`, which is exactly what a drag
    /// source does to `XdndSelection` when a drag begins.
    fn claim_selection(name: &str) -> Option<(RustConnection, u32)> {
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
            .intern_atom(false, name.as_bytes())
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

    /// An owner of a test selection that answers `TARGETS` while `offering` is
    /// set and refuses once it is cleared — what Mutter does with
    /// `XdndSelection` while it holds a Wayland drag and after it lets go.
    struct OfferingOwner {
        offering: Arc<AtomicBool>,
        stopped: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl OfferingOwner {
        fn claim(name: &str, offering: bool) -> Option<Self> {
            let (connection, _window) = claim_selection(name)?;
            let targets = intern(&connection, "TARGETS")?;
            let offering = Arc::new(AtomicBool::new(offering));
            let stopped = Arc::new(AtomicBool::new(false));
            let thread = {
                let offering = offering.clone();
                let stopped = stopped.clone();
                thread::spawn(move || {
                    serve_targets(&connection, targets, &offering, &stopped);
                })
            };
            Some(Self {
                offering,
                stopped,
                thread: Some(thread),
            })
        }

        fn stop_offering(&self) {
            self.offering.store(false, Ordering::Relaxed);
        }
    }

    impl Drop for OfferingOwner {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn serve_targets(
        connection: &RustConnection,
        targets: u32,
        offering: &AtomicBool,
        stopped: &AtomicBool,
    ) {
        while !stopped.load(Ordering::Relaxed) {
            let Ok(event) = connection.poll_for_event() else {
                return;
            };
            let Some(Event::SelectionRequest(request)) = event else {
                thread::sleep(Duration::from_millis(5));
                continue;
            };
            let property = if offering.load(Ordering::Relaxed) && request.target == targets {
                let _ = connection.change_property32(
                    PropMode::REPLACE,
                    request.requestor,
                    request.property,
                    AtomEnum::ATOM,
                    &[targets],
                );
                request.property
            } else {
                x11rb::NONE
            };
            let notify = SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: 0,
                time: request.time,
                requestor: request.requestor,
                selection: request.selection,
                target: request.target,
                property,
            };
            let _ = connection.send_event(false, request.requestor, EventMask::NO_EVENT, notify);
            let _ = connection.flush();
        }
    }

    /// Collect phases until `count` have arrived or the delivery timeout runs out.
    fn receive_phases(
        receiver: &async_channel::Receiver<DragPhase>,
        count: usize,
    ) -> Vec<DragPhase> {
        let deadline = Instant::now() + DELIVERY_TIMEOUT;
        let mut phases = Vec::new();
        while Instant::now() < deadline && phases.len() < count {
            match receiver.try_recv() {
                Ok(phase) => phases.push(phase),
                Err(_) => thread::sleep(Duration::from_millis(20)),
            }
        }
        phases
    }

    /// Manual check: claim the real `XdndSelection` so a running Yeet reveals
    /// its shelf. Ignored so it never runs in CI.
    #[test]
    #[ignore = "manual: needs a running Yeet to observe the reveal"]
    fn claim_real_xdnd_selection_for_manual_check() {
        let Some((_connection, _window)) = claim_selection(XDND_SELECTION) else {
            return;
        };
        thread::sleep(Duration::from_secs(5));
    }

    /// The whole mechanism against a real X server: a new selection owner is
    /// reported as a drag beginning, and an owner with nothing to offer — the
    /// compositor once its drag is over — as ending straight after. No pointer
    /// button is held on an unattended test machine, which is also how a drag
    /// the Wayland compositor owns looks from XWayland.
    ///
    /// Skipped where there is no X server to ask. That covers a Wayland-only
    /// session, where this backend is unavailable in exactly the same way.
    #[test]
    fn a_new_selection_owner_is_reported_as_a_drag() {
        if !available() {
            return;
        }
        let (sender, receiver) = async_channel::unbounded();
        let Some(_watch) = watch_selection(BEGIN_END_SELECTION, sender) else {
            return;
        };
        let Some(_owner) = OfferingOwner::claim(BEGIN_END_SELECTION, false) else {
            return;
        };
        assert_eq!(
            receive_phases(&receiver, 2),
            [DragPhase::Begin, DragPhase::End],
            "taking the selection should open and then close one drag"
        );
    }

    /// A drag lasts for as long as its owner still offers it, however long the
    /// pointer stays put: the pointer never moves on a test machine, just as
    /// XWayland never sees it move during a drag the compositor owns.
    #[test]
    fn a_drag_lasts_while_its_owner_still_offers_it() {
        if !available() {
            return;
        }
        let (sender, receiver) = async_channel::unbounded();
        let Some(_watch) = watch_selection(OFFERED_SELECTION, sender) else {
            return;
        };
        let Some(owner) = OfferingOwner::claim(OFFERED_SELECTION, true) else {
            return;
        };
        assert_eq!(receive_phases(&receiver, 1), [DragPhase::Begin]);
        thread::sleep(Duration::from_secs(2));
        assert!(
            receiver.try_recv().is_err(),
            "a drag that is still offered must not end"
        );
        owner.stop_offering();
        assert_eq!(
            receive_phases(&receiver, 1),
            [DragPhase::End],
            "the drag should end once the owner stops offering it"
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
        let Some(watch) = watch_selection(DROPPED_SELECTION, sender) else {
            return;
        };
        drop(watch);
        let Some((_connection, _window)) = claim_selection(DROPPED_SELECTION) else {
            return;
        };
        thread::sleep(DRAG_POLL_INTERVAL * 4);
        assert!(receiver.try_recv().is_err(), "a dropped watch stays quiet");
    }
}
