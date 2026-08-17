//! Desktop-portal integration: the global shortcut.
//!
//! Wayland gives applications no way to grab a key, so the shortcut is
//! registered through the XDG GlobalShortcuts portal. The portal backends
//! disagree with each other in ways that are invisible from the UI, which is
//! why the failure paths here are explicit and traceable with `YEET_DEBUG`.

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
