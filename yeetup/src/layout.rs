//! Where Yeet is installed, and the record of what was written.
//!
//! Files are placed one at a time under an existing prefix rather than by
//! copying whole directories over it. That is what keeps `/usr/local/share/man`
//! working when the distribution ships it as a symlink into `/usr/share/man`:
//! `create_dir_all` follows the link, while a recursive directory copy tries to
//! replace it and fails.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

/// Who the installation belongs to.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Under the user's home directory, needing no elevation.
    #[default]
    User,
    /// Machine-wide, needing root or administrator rights.
    System,
}

/// What the last install put on disk, so update and uninstall are exact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub scope: Scope,
    pub prefix: PathBuf,
    /// Prefix-relative paths, in installation order.
    pub files: Vec<PathBuf>,
}

impl Manifest {
    pub fn load() -> Result<Option<Self>> {
        let path = manifest_path()?;
        match fs::read_to_string(&path) {
            Ok(contents) => Ok(Some(serde_json::from_str(&contents)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = manifest_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn forget() -> Result<()> {
        match fs::remove_file(manifest_path()?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn manifest_path() -> Result<PathBuf> {
    let directories = directories::ProjectDirs::from("io", "hjosugi", "yeetup")
        .ok_or("no home directory is available for the install record")?;
    Ok(directories.data_dir().join("installed.json"))
}

/// The prefix an installation defaults to for a scope.
///
/// Unix keeps the `bin`/`share` split so the archive merges into an existing
/// hierarchy; Windows ships a self-contained bundle that owns its directory.
pub fn default_prefix(scope: Scope) -> Result<PathBuf> {
    if cfg!(windows) {
        let base = match scope {
            Scope::User => std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .ok_or("LOCALAPPDATA is not set")?
                .join("Programs"),
            Scope::System => std::env::var_os("ProgramFiles")
                .map(PathBuf::from)
                .ok_or("ProgramFiles is not set")?,
        };
        return Ok(base.join("Yeet"));
    }
    Ok(match scope {
        Scope::User => directories::BaseDirs::new()
            .ok_or("no home directory is available")?
            .home_dir()
            .join(".local"),
        Scope::System => PathBuf::from("/usr/local"),
    })
}

/// Where the `yeet` executable lands inside a prefix.
pub fn executable_path(prefix: &Path) -> PathBuf {
    if cfg!(windows) {
        prefix.join("yeet.exe")
    } else {
        prefix.join("bin").join("yeet")
    }
}

/// The directory that has to be on `PATH` for `yeet` to be runnable.
pub fn binary_directory(prefix: &Path) -> PathBuf {
    if cfg!(windows) {
        prefix.to_path_buf()
    } else {
        prefix.join("bin")
    }
}

/// Copy a staged tree into `prefix`, returning the prefix-relative paths written.
pub fn install_tree(staging: &Path, prefix: &Path) -> Result<Vec<PathBuf>> {
    let mut installed = Vec::new();
    copy_into(staging, prefix, Path::new(""), &mut installed)?;
    installed.sort();
    Ok(installed)
}

fn copy_into(
    source: &Path,
    prefix: &Path,
    relative: &Path,
    installed: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let child_relative = relative.join(&name);
        let child_source = entry.path();
        if entry.file_type()?.is_dir() {
            // Follows a symlinked destination component instead of replacing it.
            fs::create_dir_all(prefix.join(&child_relative))?;
            copy_into(&child_source, prefix, &child_relative, installed)?;
            continue;
        }
        let destination = prefix.join(&child_relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        // Remove first: overwriting a running executable fails on Windows and
        // would otherwise silently keep the old build.
        match fs::remove_file(&destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::copy(&child_source, &destination)?;
        installed.push(child_relative);
    }
    Ok(())
}

/// Delete the files a manifest recorded, then any directories left empty.
///
/// Only recorded paths are touched, so an install into a shared prefix such as
/// `/usr/local` never removes another package's files.
pub fn remove_installed(manifest: &Manifest) -> Result<()> {
    let mut directories: Vec<PathBuf> = Vec::new();
    for relative in &manifest.files {
        let path = manifest.prefix.join(relative);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!("could not remove {}: {error}", path.display()).into());
            }
        }
        let mut parent = relative.parent();
        while let Some(directory) = parent {
            if directory.as_os_str().is_empty() {
                break;
            }
            directories.push(manifest.prefix.join(directory));
            parent = directory.parent();
        }
    }
    // Deepest first, so a directory is only considered once its children are gone.
    directories.sort();
    directories.dedup();
    for directory in directories.into_iter().rev() {
        let _ = fs::remove_dir(&directory);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installing_through_a_symlinked_directory_writes_to_its_target() {
        // Reproduces the Arch layout that broke the documented `cp -a` install:
        // `<prefix>/share/man` is a symlink to a directory elsewhere.
        let temporary = tempfile::tempdir().unwrap();
        let staging = temporary.path().join("staging");
        let prefix = temporary.path().join("prefix");
        let real_man = temporary.path().join("real-man");

        fs::create_dir_all(staging.join("share/man/man1")).unwrap();
        fs::write(staging.join("share/man/man1/yeet.1"), b"manual").unwrap();
        fs::create_dir_all(prefix.join("share")).unwrap();
        fs::create_dir_all(&real_man).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_man, prefix.join("share/man")).unwrap();

        let installed = install_tree(&staging, &prefix).unwrap();

        assert_eq!(installed, vec![PathBuf::from("share/man/man1/yeet.1")]);
        #[cfg(unix)]
        assert_eq!(
            fs::read(real_man.join("man1/yeet.1")).unwrap(),
            b"manual",
            "the file should land in the symlink's target"
        );
    }

    #[test]
    fn uninstall_removes_recorded_files_and_leaves_foreign_ones() {
        let temporary = tempfile::tempdir().unwrap();
        let prefix = temporary.path().join("prefix");
        fs::create_dir_all(prefix.join("bin")).unwrap();
        fs::write(prefix.join("bin/yeet"), b"binary").unwrap();
        fs::write(prefix.join("bin/other-tool"), b"not ours").unwrap();

        let manifest = Manifest {
            version: "0.5.3".to_owned(),
            scope: Scope::User,
            prefix: prefix.clone(),
            files: vec![PathBuf::from("bin/yeet")],
        };
        remove_installed(&manifest).unwrap();

        assert!(!prefix.join("bin/yeet").exists());
        assert!(prefix.join("bin/other-tool").exists());
        assert!(
            prefix.join("bin").exists(),
            "a shared directory must survive"
        );
    }

    #[test]
    fn empty_directories_created_by_the_install_are_cleaned_up() {
        let temporary = tempfile::tempdir().unwrap();
        let prefix = temporary.path().join("prefix");
        fs::create_dir_all(prefix.join("share/metainfo")).unwrap();
        fs::write(prefix.join("share/metainfo/yeet.xml"), b"<x/>").unwrap();

        let manifest = Manifest {
            version: "0.5.3".to_owned(),
            scope: Scope::User,
            prefix: prefix.clone(),
            files: vec![PathBuf::from("share/metainfo/yeet.xml")],
        };
        remove_installed(&manifest).unwrap();

        assert!(!prefix.join("share/metainfo").exists());
    }
}
