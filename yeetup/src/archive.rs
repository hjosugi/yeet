//! Archive unpacking.
//!
//! Both release containers wrap their payload in a single versioned directory
//! (`yeet-0.5.3-linux-x86_64/…`). That component is stripped here so the rest
//! of the installer only ever deals with prefix-relative paths like
//! `bin/yeet` and `share/man/man1/yeet.1`.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::Result;
use crate::release::Container;

pub fn unpack(archive: &Path, container: Container, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    match container {
        Container::TarGz => unpack_tar_gz(archive, destination),
        Container::Zip => unpack_zip(archive, destination),
    }
}

fn unpack_tar_gz(archive: &Path, destination: &Path) -> Result<()> {
    let file = fs::File::open(archive)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let Some(relative) = strip_root(&entry.path()?) else {
            continue;
        };
        let target = safe_join(destination, &relative)?;
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(&target)?;
    }
    Ok(())
}

fn unpack_zip(archive: &Path, destination: &Path) -> Result<()> {
    let file = fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        // `enclosed_name` is `None` for entries that try to escape the archive
        // root, which is exactly the case worth refusing.
        let Some(name) = entry.enclosed_name() else {
            return Err(format!("archive entry {} has an unsafe path", entry.name()).into());
        };
        let Some(relative) = strip_root(&name) else {
            continue;
        };
        let target = safe_join(destination, &relative)?;
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::File::create(&target)?;
        io::copy(&mut entry, &mut output)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

/// Drop the archive's single top-level directory.
///
/// Returns `None` for the top-level entry itself, which has nothing left after
/// stripping and so needs no action.
fn strip_root(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    components.next()?;
    let rest: PathBuf = components.collect();
    (!rest.as_os_str().is_empty()).then_some(rest)
}

/// Join a relative archive path onto `root`, refusing anything that escapes it.
///
/// Archives are downloaded from the network, so `..` traversal and absolute
/// paths are rejected outright instead of being normalised into something that
/// merely looks safe.
fn safe_join(root: &Path, relative: &Path) -> Result<PathBuf> {
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(format!(
                    "archive entry {} escapes the extraction directory",
                    relative.display()
                )
                .into());
            }
        }
    }
    Ok(root.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_versioned_top_level_directory_is_removed() {
        assert_eq!(
            strip_root(Path::new("yeet-0.5.3-linux-x86_64/bin/yeet")),
            Some(PathBuf::from("bin/yeet"))
        );
        assert_eq!(
            strip_root(Path::new("yeet-0.5.3-linux-x86_64/share/man/man1/yeet.1")),
            Some(PathBuf::from("share/man/man1/yeet.1"))
        );
        assert_eq!(strip_root(Path::new("yeet-0.5.3-linux-x86_64")), None);
    }

    #[test]
    fn traversal_and_absolute_entries_are_refused() {
        let root = Path::new("/tmp/staging");

        assert!(safe_join(root, Path::new("bin/yeet")).is_ok());
        assert!(safe_join(root, Path::new("../outside")).is_err());
        assert!(safe_join(root, Path::new("bin/../../outside")).is_err());
        assert!(safe_join(root, Path::new("/etc/passwd")).is_err());
    }
}
