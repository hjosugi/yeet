//! Build and package Yeet's release artifacts.
//!
//! One Rust entry point for every target so the naming, layout and checksum
//! rules live in a single place instead of being restated by a shell script per
//! operating system. `yeetup` consumes exactly what this produces.

mod appimage;
mod pack;
mod stage;

use std::path::{Path, PathBuf};
use std::process::Command;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const HELP: &str = "\
Usage: cargo xtask <COMMAND> [ARGS]

Commands:
  package <TARGET>   Build a release artifact into dist/
  checksums <FAMILY> Write SHA256SUMS-<FAMILY>.txt for dist/ artifacts
  version            Print the version taken from Cargo.toml

Targets:
  linux-tar     yeet-<version>-linux-<arch>.tar.gz
  appimage      yeet-<version>-linux-<arch>.AppImage
  windows-zip   yeet-<version>-windows-x64.zip from an existing bundle directory
  macos-dmg     yeet-<version>-macos-<arch>.dmg

Families:
  linux, windows, macos
";

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().unwrap_or_else(|| "--help".to_owned());
    match command.as_str() {
        "--help" | "-h" | "help" => {
            print!("{HELP}");
            Ok(())
        }
        "version" => {
            println!("{}", version()?);
            Ok(())
        }
        "package" => {
            let target = arguments.next().ok_or("package needs a target")?;
            package(&target)
        }
        "checksums" => {
            let family = arguments.next().ok_or("checksums needs a family")?;
            checksums(&family)
        }
        other => Err(format!("unknown command {other}; run `cargo xtask --help`").into()),
    }
}

/// The repository root, derived from this crate rather than the caller's
/// working directory so `cargo xtask` works from anywhere in the tree.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

pub fn dist_dir() -> Result<PathBuf> {
    let directory = workspace_root().join("dist");
    std::fs::create_dir_all(&directory)?;
    Ok(directory)
}

/// Read the shipped version from the application's manifest.
///
/// Parsed rather than taken from `CARGO_PKG_VERSION` so xtask reports the
/// application's version even if the two crates ever diverge.
pub fn version() -> Result<String> {
    let manifest = std::fs::read_to_string(workspace_root().join("Cargo.toml"))?;
    manifest
        .lines()
        .skip_while(|line| line.trim() != "[package]")
        .find_map(|line| line.strip_prefix("version = "))
        .map(|value| value.trim().trim_matches('"').to_owned())
        .ok_or_else(|| "no package version in Cargo.toml".into())
}

/// The architecture as it appears in artifact names.
fn arch() -> &'static str {
    std::env::consts::ARCH
}

pub fn build_release_binary() -> Result<PathBuf> {
    println!("Building the release binary…");
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()))
        .current_dir(workspace_root())
        .args(["build", "--release", "--locked", "-p", "yeet"])
        .status()?;
    if !status.success() {
        return Err("cargo build --release failed".into());
    }
    let binary =
        target_dir()?
            .join("release")
            .join(if cfg!(windows) { "yeet.exe" } else { "yeet" });
    if !binary.exists() {
        return Err(format!("{} was not produced", binary.display()).into());
    }
    Ok(binary)
}

/// Honour `CARGO_TARGET_DIR`, which developers and CI caches both set.
pub fn target_dir() -> Result<PathBuf> {
    Ok(match std::env::var_os("CARGO_TARGET_DIR") {
        Some(directory) => PathBuf::from(directory),
        None => workspace_root().join("target"),
    })
}

fn package(target: &str) -> Result<()> {
    let version = version()?;
    match target {
        "linux-tar" => linux_tar(&version),
        "appimage" => appimage::build(&version),
        "windows-zip" => windows_zip(&version),
        "macos-dmg" => macos_dmg(&version),
        other => Err(format!("unknown target {other}; run `cargo xtask --help`").into()),
    }
}

fn linux_tar(version: &str) -> Result<()> {
    let binary = build_release_binary()?;
    let root = workspace_root();
    let name = format!("yeet-{version}-linux-{}", arch());
    let staging = target_dir()?.join("xtask").join(&name);
    stage::stage(&stage::unix_payload(&root, &binary), &staging)?;
    let archive = dist_dir()?.join(format!("{name}.tar.gz"));
    pack::tar_gz(&staging, &archive)?;
    println!("{}", archive.display());
    Ok(())
}

/// Zip the portable Windows bundle produced by `scripts/bundle-windows.sh`.
///
/// The bundle itself is assembled inside MSYS2, where `ldd` can resolve the
/// UCRT64 DLLs; xtask only turns that directory into the published archive.
fn windows_zip(version: &str) -> Result<()> {
    let name = format!("yeet-{version}-windows-x64");
    let bundle = workspace_root().join(&name);
    if !bundle.is_dir() {
        return Err(format!(
            "{} does not exist; run scripts/bundle-windows.sh first",
            bundle.display()
        )
        .into());
    }
    let archive = dist_dir()?.join(format!("{name}.zip"));
    pack::zip(&bundle, &archive)?;
    println!("{}", archive.display());
    Ok(())
}

/// Build a `.app` bundle and wrap it in a disk image.
///
/// Yeet has no macOS platform backend yet, so this produces a runnable GTK
/// build without the always-on-top shelf, tray or global shortcut that the
/// Linux and Windows backends provide.
fn macos_dmg(version: &str) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Err("macos-dmg has to run on macOS: it uses hdiutil".into());
    }
    let binary = build_release_binary()?;
    let name = format!("yeet-{version}-macos-{}", arch());
    let staging = target_dir()?.join("xtask").join(&name);
    let app = staging.join("Yeet.app");
    let macos_dir = app.join("Contents/MacOS");
    let resources = app.join("Contents/Resources");
    std::fs::create_dir_all(&macos_dir)?;
    std::fs::create_dir_all(&resources)?;
    std::fs::copy(&binary, macos_dir.join("yeet"))?;
    std::fs::copy(
        workspace_root().join("assets/io.github.hjosugi.Yeet.svg"),
        resources.join("io.github.hjosugi.Yeet.svg"),
    )?;
    std::fs::write(app.join("Contents/Info.plist"), info_plist(version))?;

    let image = dist_dir()?.join(format!("{name}.dmg"));
    let _ = std::fs::remove_file(&image);
    let status = Command::new("hdiutil")
        .args(["create", "-volname", "Yeet", "-srcfolder"])
        .arg(&staging)
        .args(["-ov", "-format", "UDZO"])
        .arg(&image)
        .status()?;
    if !status.success() {
        return Err("hdiutil create failed".into());
    }
    println!("{}", image.display());
    Ok(())
}

fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Yeet</string>
  <key>CFBundleDisplayName</key><string>Yeet</string>
  <key>CFBundleIdentifier</key><string>io.github.hjosugi.Yeet</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleExecutable</key><string>yeet</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
    )
}

fn checksums(family: &str) -> Result<()> {
    if !matches!(family, "linux" | "windows" | "macos") {
        return Err(format!("unknown family {family}").into());
    }
    let dist = dist_dir()?;
    let mut artifacts: Vec<PathBuf> = std::fs::read_dir(&dist)?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && belongs_to(path, family))
        .collect();
    if artifacts.is_empty() {
        return Err(format!("no {family} artifacts in {}", dist.display()).into());
    }
    artifacts.sort();
    let manifest = dist.join(format!("SHA256SUMS-{family}.txt"));
    pack::write_checksums(&artifacts, &manifest)?;
    println!("{}", manifest.display());
    Ok(())
}

fn belongs_to(path: &Path, family: &str) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    !name.starts_with("SHA256SUMS") && name.contains(&format!("-{family}-"))
}
