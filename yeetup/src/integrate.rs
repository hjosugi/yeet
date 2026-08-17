//! Post-install desktop integration.
//!
//! None of this is required for `yeet` to run, so every step is best effort and
//! reports rather than fails: an installer that aborts because an icon cache
//! could not be refreshed is worse than one that says so and finishes.

use std::path::Path;
use std::process::Command;

use crate::Result;
use crate::layout;

/// Refresh the caches a desktop reads, and report when `PATH` needs attention.
pub fn finish(prefix: &Path) -> Result<()> {
    let binaries = layout::binary_directory(prefix);
    if !path_contains(&binaries) {
        println!(
            "\nNote: {} is not on your PATH.\n  Add it with:\n    {}",
            binaries.display(),
            path_advice(&binaries)
        );
    }
    refresh_caches(prefix);
    #[cfg(windows)]
    add_to_windows_path(&binaries);
    Ok(())
}

/// Rebuild the desktop caches for a prefix.
///
/// Also run after an uninstall: the caches keep listing a removed application
/// until something regenerates them.
pub fn refresh_caches(prefix: &Path) {
    #[cfg(target_os = "linux")]
    refresh_linux_caches(prefix);
    #[cfg(not(target_os = "linux"))]
    let _ = prefix;
}

fn path_contains(directory: &Path) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|entry| entry == directory))
}

fn path_advice(directory: &Path) -> String {
    if cfg!(windows) {
        format!(
            "yeetup added it to your user PATH; open a new terminal to pick up {}",
            directory.display()
        )
    } else {
        format!("export PATH=\"{}:$PATH\"", directory.display())
    }
}

/// Update the desktop entry and icon caches so Yeet shows up in the app grid.
#[cfg(target_os = "linux")]
fn refresh_linux_caches(prefix: &Path) {
    let applications = prefix.join("share/applications");
    if applications.is_dir() {
        run_quietly("update-desktop-database", &[applications.as_os_str()]);
    }
    let icons = prefix.join("share/icons/hicolor");
    if icons.is_dir() {
        run_quietly(
            "gtk-update-icon-cache",
            &["-qtf".as_ref(), icons.as_os_str()],
        );
    }
}

#[cfg(target_os = "linux")]
fn run_quietly(program: &str, arguments: &[&std::ffi::OsStr]) {
    // A missing tool is normal on a minimal system and not worth reporting.
    let _ = Command::new(program).args(arguments).status();
}

/// Append the install directory to the user `PATH`.
///
/// Done through PowerShell's environment API rather than `setx`, which
/// truncates `PATH` at 1024 characters and has corrupted user environments.
#[cfg(windows)]
fn add_to_windows_path(directory: &Path) {
    let directory = directory.display().to_string();
    let script = format!(
        "$target = '{}';
         $current = [Environment]::GetEnvironmentVariable('Path', 'User');
         if ($null -eq $current) {{ $current = '' }}
         if (($current -split ';') -notcontains $target) {{
             $updated = if ($current.TrimEnd(';') -eq '') {{ $target }} else {{ $current.TrimEnd(';') + ';' + $target }}
             [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
         }}",
        directory.replace('\'', "''")
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status();
    if !matches!(status, Ok(status) if status.success()) {
        println!("Note: could not update your PATH automatically; add {directory} manually.");
    }
}
