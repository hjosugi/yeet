//! Atomic replacement of the small JSON state files Yeet owns.
//!
//! Both the shelf state and the settings file are written whole and read
//! whole, so a reader must never observe a half-written file. The write goes
//! to a sibling temporary first and is renamed into place, which is atomic on
//! every platform Yeet targets. Windows refuses to rename over an existing
//! file, so the destination is removed immediately before the rename there.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Write `data` to `path`, replacing whatever was there in one step.
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = temporary_path(path);
    fs::write(&temporary, data)?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)
}

/// The sibling file a write lands in before it is renamed over `path`.
///
/// The whole file name is kept and `.tmp` appended, so `state.json` is staged
/// as `state.json.tmp` rather than `state.tmp`.
fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_replaces_the_previous_contents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");

        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"second");
    }

    #[test]
    fn the_parent_directory_is_created_on_demand() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/deeper/state.json");

        write_atomic(&path, b"value").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"value");
    }

    #[test]
    fn no_temporary_file_is_left_behind() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");

        write_atomic(&path, b"value").unwrap();

        assert!(!temporary_path(&path).exists());
        assert_eq!(
            temporary_path(&path),
            directory.path().join("state.json.tmp")
        );
    }
}
