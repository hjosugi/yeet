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
        let name = entry.name()?;
        if name.starts_with(['/', '\\']) || name.as_bytes().get(1) == Some(&b':') {
            return Err(format!("archive entry {name} has an unsafe path").into());
        }
        // `enclosed_name` is `None` for entries that try to escape the archive
        // root, which is exactly the case worth refusing.
        let Some(name) = entry.enclosed_name() else {
            return Err(format!("archive entry {} has an unsafe path", entry.name()?).into());
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

            fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777))?;
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
    fn zip_extraction_rejects_absolute_names_before_library_normalization() {
        use std::io::{Cursor, Write};

        for name in ["/etc/yeet", "C:/Windows/yeet", "\\\\server\\share\\yeet"] {
            let fixture = tempfile::tempdir().unwrap();
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"untrusted").unwrap();
            let archive = fixture.path().join("absolute.zip");
            fs::write(&archive, writer.finish().unwrap().into_inner()).unwrap();
            assert!(unpack(&archive, Container::Zip, &fixture.path().join("out")).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn zip_extraction_preserves_executable_bits_without_special_permissions() {
        use std::io::{Cursor, Write};
        use std::os::unix::fs::PermissionsExt;

        let fixture = tempfile::tempdir().unwrap();
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("release/bin/yeet", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"executable payload").unwrap();
        let mut bytes = writer.finish().unwrap().into_inner();
        // Model an untrusted archive: the writer itself strips special bits.
        let central = bytes.windows(4).position(|x| x == b"PK\x01\x02").unwrap();
        bytes[central + 38..central + 42].copy_from_slice(&((0o106755_u32) << 16).to_le_bytes());
        let archive = fixture.path().join("release.zip");
        fs::write(&archive, bytes).unwrap();
        let destination = fixture.path().join("out");
        unpack(&archive, Container::Zip, &destination).unwrap();

        let output = destination.join("bin/yeet");
        assert_eq!(fs::read(&output).unwrap(), b"executable payload");
        assert_eq!(
            fs::metadata(output).unwrap().permissions().mode() & 0o7777,
            0o755
        );
    }

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
