//! AppImage packaging.
//!
//! A single self-contained file is the one Linux artifact that works on every
//! distribution regardless of its GTK version, which is what makes it worth the
//! extra machinery over the tarball.
//!
//! Bundling GTK 4 correctly means more than copying the libraries `ldd` reports:
//! gdk-pixbuf loaders, GSettings schemas, GIRepository typelibs and the icon
//! theme all have to travel too, and the launcher has to point GTK at them.
//! `linuxdeploy` with its GTK plugin already encodes that knowledge, so xtask
//! builds the AppDir and hands the runtime bundling to it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Result, build_release_binary, dist_dir, stage, target_dir, workspace_root};

const LINUXDEPLOY_URL: &str = "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage";
const GTK_PLUGIN_URL: &str = "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh";

pub fn build(version: &str) -> Result<()> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(format!(
            "AppImage packaging is wired for x86_64; {} needs its own linuxdeploy build",
            std::env::consts::ARCH
        )
        .into());
    }
    let binary = build_release_binary()?;
    let root = workspace_root();
    let appdir = target_dir()?.join("xtask").join("AppDir");

    // linuxdeploy expects the FHS layout under `usr`, and finds the desktop
    // entry, icon and binary from there.
    let payload = stage::unix_payload(&root, &binary)
        .into_iter()
        .map(|mut item| {
            item.destination = PathBuf::from("usr").join(item.destination);
            item
        })
        .collect::<Vec<_>>();
    stage::stage(&payload, &appdir)?;
    // The top-level icon and desktop entry are what the AppImage runtime and
    // desktop integration read; linuxdeploy copies them from `usr` only when
    // they are named on the command line, which is what happens below.
    fs::copy(
        root.join("assets/io.github.hjosugi.Yeet.svg"),
        appdir.join("io.github.hjosugi.Yeet.svg"),
    )?;

    let tools = target_dir()?.join("xtask").join("tools");
    fs::create_dir_all(&tools)?;
    let linuxdeploy = ensure_tool(&tools, "linuxdeploy-x86_64.AppImage", LINUXDEPLOY_URL)?;
    let gtk_plugin = ensure_tool(&tools, "linuxdeploy-plugin-gtk.sh", GTK_PLUGIN_URL)?;

    let output_name = format!("yeet-{version}-linux-x86_64.AppImage");
    println!("Bundling the GTK runtime with linuxdeploy…");
    let status = Command::new(&linuxdeploy)
        .current_dir(target_dir()?.join("xtask"))
        // linuxdeploy discovers plugins from PATH by their file name.
        .env("PATH", prepend_path(&tools)?)
        .env("OUTPUT", &output_name)
        // Extract rather than mount: CI containers have no FUSE.
        .env("APPIMAGE_EXTRACT_AND_RUN", "1")
        .arg("--appdir")
        .arg(&appdir)
        .arg("--plugin")
        .arg("gtk")
        .arg("--desktop-file")
        .arg(appdir.join("usr/share/applications/io.github.hjosugi.Yeet.desktop"))
        .arg("--icon-file")
        .arg(appdir.join("io.github.hjosugi.Yeet.svg"))
        .arg("--executable")
        .arg(appdir.join("usr/bin/yeet"))
        .arg("--output")
        .arg("appimage")
        .status()
        .map_err(|error| format!("could not run {}: {error}", linuxdeploy.display()))?;
    if !status.success() {
        return Err("linuxdeploy failed to build the AppImage".into());
    }
    let _ = gtk_plugin;

    let produced = target_dir()?.join("xtask").join(&output_name);
    let destination = dist_dir()?.join(&output_name);
    move_file(&produced, &destination)?;
    println!("{}", destination.display());
    Ok(())
}

/// Move a file, falling back to copy-then-delete across filesystems.
///
/// `CARGO_TARGET_DIR` is routinely pointed at a different volume from the
/// checkout, and a plain rename fails there with `EXDEV`.
fn move_file(from: &Path, to: &Path) -> Result<()> {
    match fs::rename(from, to) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {}
        Err(error) => return Err(format!("moving {}: {error}", from.display()).into()),
    }
    fs::copy(from, to).map_err(|error| format!("copying {}: {error}", from.display()))?;
    make_executable(to)?;
    fs::remove_file(from)?;
    Ok(())
}

/// Download a helper once and keep it in the target directory.
///
/// Cached rather than fetched every run so repeated local packaging does not
/// depend on the network, and so CI caching of `target/` covers it too.
fn ensure_tool(directory: &Path, name: &str, url: &str) -> Result<PathBuf> {
    let path = directory.join(name);
    if path.exists() {
        return Ok(path);
    }
    println!("Downloading {name}…");
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&path)
        .arg(url)
        .status()
        .map_err(|error| format!("curl is required to fetch {name}: {error}"))?;
    if !status.success() {
        let _ = fs::remove_file(&path);
        return Err(format!("could not download {name} from {url}").into());
    }
    make_executable(&path)?;
    Ok(path)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn prepend_path(directory: &Path) -> Result<std::ffi::OsString> {
    let existing = std::env::var_os("PATH").unwrap_or_default();
    let mut entries = vec![directory.to_path_buf()];
    entries.extend(std::env::split_paths(&existing));
    Ok(std::env::join_paths(entries)?)
}
