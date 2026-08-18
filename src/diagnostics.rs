//! What Yeet leaves behind when it fails to start.
//!
//! Release builds on Windows use the GUI subsystem, so a process that dies
//! before its window appears writes to a stderr nobody is reading and simply
//! disappears from the screen — the user sees a launch that does nothing.
//! Every failure that reaches here is therefore appended to a file they can be
//! pointed at, and on Windows also shown once in a dialog, because "it closes
//! instantly" is not something anyone can act on.
//!
//! Nothing here may fail loudly in turn: this runs on the path where Yeet is
//! already broken, so every error is dropped rather than reported.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Keep a failing session from growing the log without bound.
const MAX_LOG_BYTES: u64 = 64 * 1024;

/// How many GTK messages one run may record.
///
/// A broken driver can emit a warning per frame. The first few say everything
/// a repeat says, and a resident application must not answer them with a file
/// write each time.
const MAX_CAPTURED_MESSAGES: usize = 200;

/// Only the first failure gets a dialog; the rest only reach the log.
static ALERTED: AtomicBool = AtomicBool::new(false);

/// GTK messages recorded so far this run.
static CAPTURED: AtomicUsize = AtomicUsize::new(0);

/// Start recording failures, and report where they will be recorded.
pub fn install() -> Option<PathBuf> {
    let path = log_path()?;
    let hook_path = path.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // `info` already renders the payload and the source location.
        record(&hook_path, &format!("panicked: {info}"));
        alert(&format!(
            "Yeet stopped unexpectedly.\n\n{info}\n\nDetails were written to\n{}",
            hook_path.display()
        ));
        previous(info);
    }));
    capture_gtk_warnings(path.clone());
    Some(path)
}

/// Copy GTK's own warnings into the log on their way to stderr.
///
/// The panic hook cannot cover the worst start-up failure: when GTK cannot
/// open a display it prints "Failed to open display" and ends the process from
/// inside the C library, so no Rust code downstream of `run` ever executes.
/// The message itself is the whole diagnosis, and this is the only place it
/// can be caught.
fn capture_gtk_warnings(path: PathBuf) {
    glib::log_set_writer_func(move |level, fields| {
        if matches!(
            level,
            glib::LogLevel::Error | glib::LogLevel::Critical | glib::LogLevel::Warning
        ) {
            let message = fields
                .iter()
                .find(|field| field.key() == "MESSAGE")
                .and_then(|field| field.value_str());
            if let Some(message) = message {
                let domain = fields
                    .iter()
                    .find(|field| field.key() == "GLIB_DOMAIN")
                    .and_then(|field| field.value_str())
                    .unwrap_or("GLib");
                match CAPTURED.fetch_add(1, Ordering::Relaxed) {
                    count if count < MAX_CAPTURED_MESSAGES => {
                        record(&path, &format!("{domain}: {message}"));
                    }
                    count if count == MAX_CAPTURED_MESSAGES => record(
                        &path,
                        "further GTK messages this run are not recorded; \
                         they are still on stderr",
                    ),
                    _ => {}
                }
            }
        }
        // Leave the terminal's view of the session exactly as it was.
        glib::log_writer_default(level, fields)
    });
}

/// Record that the GTK application gave up before showing anything.
///
/// The message GTK printed went to stderr, which on a Windows launch from
/// Explorer or the tray is nowhere at all.
pub fn record_startup_failure(log: Option<&Path>, code: u8) {
    let detail = format!(
        "the GTK application exited with code {code} before the shelf was \
         ready. Session: {}",
        session_summary()
    );
    if let Some(log) = log {
        record(log, &detail);
    }
    // Recorded, never shown. This is the path a scripted `yeet --toggle` takes
    // when the running instance reports a failure, and a modal dialog would
    // hold the caller open instead of letting it see the exit code.
    eprintln!("yeet: {detail}");
}

/// Append one timestamped line, trimming the log if it has grown large.
fn record(path: &Path, message: &str) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let oversized = std::fs::metadata(path).is_ok_and(|data| data.len() > MAX_LOG_BYTES);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(!oversized)
        .write(true)
        .truncate(oversized)
        .open(path)
    else {
        return;
    };
    let now = glib::DateTime::now_local()
        .and_then(|now| now.format("%Y-%m-%d %H:%M:%S"))
        .map(|now| now.to_string())
        .unwrap_or_else(|_| "unknown time".to_owned());
    let _ = writeln!(file, "{now} yeet {}: {message}", env!("CARGO_PKG_VERSION"));
}

/// The parts of the session that decide how Yeet starts.
fn session_summary() -> String {
    let variables = [
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "GDK_BACKEND",
        "YEET_BACKEND",
    ];
    let described: Vec<String> = variables
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| format!("{name}={value}"))
        })
        .collect();
    if described.is_empty() {
        return "no display environment is set".to_owned();
    }
    described.join(" ")
}

/// Put the first failure of a run in front of the user.
///
/// Only when there is nowhere else for it to appear. A Yeet started from a
/// terminal has already printed the same text there, and a modal dialog in
/// front of a script or a test run is worse than useless — it holds the
/// process open. The launch this exists for is the one from Explorer, a
/// shortcut or the tray, which has no console at all.
#[cfg(target_os = "windows")]
fn alert(message: &str) {
    use windows::Win32::System::Console::GetConsoleWindow;
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::PCWSTR;

    // SAFETY: no arguments, and the returned handle is only tested for null.
    if !unsafe { GetConsoleWindow() }.is_invalid() {
        return;
    }
    if ALERTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let caption: Vec<u16> = "Yeet".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: both strings are NUL-terminated and outlive the modal call.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_OK | MB_ICONERROR,
        )
    };
}

/// Elsewhere the terminal that started Yeet is still there to read.
#[cfg(not(target_os = "windows"))]
fn alert(_message: &str) {
    ALERTED.store(true, Ordering::Relaxed);
}

/// Where failures are recorded, beside the shelf's own state.
fn log_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("io", "hjosugi", "Yeet")
        .map(|directories| directories.data_local_dir().join("yeet.log"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_running_failure_cannot_grow_the_log_without_bound() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/yeet.log");

        record(&path, "first");
        assert!(
            std::fs::read_to_string(&path).unwrap().contains("first"),
            "the log directory is created on demand"
        );

        record(&path, "second");
        let both = std::fs::read_to_string(&path).unwrap();
        assert!(
            both.contains("first") && both.contains("second"),
            "failures accumulate rather than replacing each other"
        );

        std::fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize + 1]).unwrap();
        record(&path, "after the cap");
        let trimmed = std::fs::read_to_string(&path).unwrap();
        assert!(trimmed.contains("after the cap"));
        assert!(
            (trimmed.len() as u64) < MAX_LOG_BYTES,
            "an oversized log is replaced, not appended to"
        );
    }

    #[test]
    fn the_session_summary_names_the_variables_that_are_set() {
        // SAFETY: single-threaded test process, and the value is restored.
        unsafe { std::env::set_var("YEET_BACKEND", "x11") };
        assert!(session_summary().contains("YEET_BACKEND=x11"));
        unsafe { std::env::remove_var("YEET_BACKEND") };
    }
}
