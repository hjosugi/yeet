//! The payload layout shared by every Linux and macOS artifact.
//!
//! The tarball, the AppImage's AppDir and the macOS bundle all start from the
//! same set of files. Describing that set once means a new file (a new icon
//! size, another metainfo document) reaches every artifact at the same time.

use std::fs;
use std::path::{Path, PathBuf};

use crate::Result;

/// One file to ship: where it comes from, where it goes, and whether it runs.
pub struct Payload {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub executable: bool,
}

/// The prefix-relative layout installed on Unix hosts.
///
/// Mirrors what `yeetup` expects to unpack, so the two stay in step.
pub fn unix_payload(root: &Path, binary: &Path) -> Vec<Payload> {
    let file = |source: PathBuf, destination: &str| Payload {
        source,
        destination: PathBuf::from(destination),
        executable: false,
    };
    vec![
        Payload {
            source: binary.to_path_buf(),
            destination: PathBuf::from("bin/yeet"),
            executable: true,
        },
        file(
            root.join("packaging/linux/io.github.hjosugi.Yeet.desktop"),
            "share/applications/io.github.hjosugi.Yeet.desktop",
        ),
        file(
            root.join("packaging/linux/io.github.hjosugi.Yeet.metainfo.xml"),
            "share/metainfo/io.github.hjosugi.Yeet.metainfo.xml",
        ),
        file(
            root.join("assets/io.github.hjosugi.Yeet.svg"),
            "share/icons/hicolor/scalable/apps/io.github.hjosugi.Yeet.svg",
        ),
        file(root.join("packaging/linux/yeet.1"), "share/man/man1/yeet.1"),
        file(root.join("LICENSE"), "share/licenses/yeet/LICENSE"),
    ]
}

/// Materialise a payload under `destination_root`.
pub fn stage(payload: &[Payload], destination_root: &Path) -> Result<()> {
    if destination_root.exists() {
        fs::remove_dir_all(destination_root)?;
    }
    for item in payload {
        let target = destination_root.join(&item.destination);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&item.source, &target)
            .map_err(|error| format!("staging {}: {error}", item.source.display()))?;
        set_mode(&target, item.executable)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _executable: bool) -> Result<()> {
    Ok(())
}
